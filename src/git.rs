use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Unmerged,
}

impl Status {
    pub fn symbol(self) -> char {
        match self {
            Status::Modified => 'M',
            Status::Added => 'A',
            Status::Deleted => 'D',
            Status::Renamed => 'R',
            Status::Untracked => '?',
            Status::Unmerged => 'U',
        }
    }
}

type NumStat = HashMap<String, (Option<u32>, Option<u32>)>;

#[derive(Clone, Debug)]
pub struct FileChange {
    pub path: String,
    pub status: Status,
    pub insertions: Option<u32>,
    pub deletions: Option<u32>,
}

pub struct Snapshot {
    pub branch: Option<String>,
    pub files: Vec<FileChange>,
}

pub fn repo_root() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        anyhow::bail!("not inside a git repository");
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

/// Absolute path to this checkout's git directory. For a linked worktree this
/// lives outside the worktree root (e.g. `main/.git/worktrees/<name>`), so it
/// must be watched separately from the worktree files.
pub fn git_dir(root: &Path) -> Result<PathBuf> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .context("failed to run git rev-parse --absolute-git-dir")?;
    if !out.status.success() {
        anyhow::bail!("failed to resolve git directory");
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim(),
    ))
}

pub fn collect(root: &Path) -> Result<Snapshot> {
    let counts = numstat(root)?;
    let (branch, files) = status(root, &counts)?;
    Ok(Snapshot { branch, files })
}

/// Tracked files that match ignore rules (force-added at some point). Ignore
/// rules only apply to untracked files, so edits to these still change status
/// and must bypass the watcher's ignore filter.
pub fn tracked_ignored(root: &Path) -> Result<HashSet<PathBuf>> {
    let out = Command::new("git")
        .current_dir(root)
        .args([
            "--no-optional-locks",
            "ls-files",
            "-c",
            "-i",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .context("failed to run git ls-files")?;
    if !out.status.success() {
        return Ok(HashSet::new());
    }
    Ok(out
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(String::from_utf8_lossy(s).into_owned()))
        .collect())
}

fn numstat(root: &Path) -> Result<NumStat> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["--no-optional-locks", "diff", "--numstat", "HEAD"])
        .output()
        .context("failed to run git diff")?;
    let mut map = HashMap::new();
    if !out.status.success() {
        // No HEAD yet (empty repo) — counts simply unavailable.
        return Ok(map);
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(ins), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let ins = ins.parse::<u32>().ok();
        let del = del.parse::<u32>().ok();
        map.insert(path.to_string(), (ins, del));
    }
    Ok(map)
}

fn status(root: &Path, counts: &NumStat) -> Result<(Option<String>, Vec<FileChange>)> {
    let out = Command::new("git")
        .current_dir(root)
        .args([
            // Skip the index refresh git status does by default — avoids
            // contending for .git/index.lock with a concurrent `git commit`
            // (this runs on every file-system change `changed` observes).
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            // "--branch" adds "# branch.head <name>" header lines, so the
            // branch name comes free instead of costing a second subprocess.
            "--branch",
            // "normal" (not "all") lets git use the untracked-cache/fsmonitor
            // shortcuts for whole untracked directories — "all" forces a full
            // recursive file-by-file listing and is dramatically slower on
            // large repos regardless of those caches being enabled.
            "--untracked-files=normal",
            "--no-renames",
        ])
        .output()
        .context("failed to run git status")?;
    if !out.status.success() {
        anyhow::bail!("git status failed");
    }

    let mut branch = None;
    let mut files = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(name) = line.strip_prefix("# branch.head ") {
            let name = name.trim();
            branch = if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            };
            continue;
        }
        let Some((tag, rest)) = line.split_once(' ') else {
            continue;
        };
        let (path, status) = match tag {
            "1" => {
                // <xy> <sub> <mH> <mI> <mW> <hH> <hI> <path>
                let fields: Vec<&str> = rest.splitn(8, ' ').collect();
                if fields.len() < 8 {
                    continue;
                }
                (fields[7].to_string(), classify(fields[0]))
            }
            "2" => {
                // <xy> ... <Xscore> <path>\t<orig>
                let fields: Vec<&str> = rest.splitn(9, ' ').collect();
                if fields.len() < 9 {
                    continue;
                }
                let path = fields[8].split('\t').next().unwrap_or("").to_string();
                (path, Status::Renamed)
            }
            "?" => (rest.to_string(), Status::Untracked),
            "u" => {
                // Rebase/merge conflicts — porcelain v1 used "UU path"; v2 uses "u … path".
                let fields: Vec<&str> = rest.split_whitespace().collect();
                if fields.len() < 2 {
                    continue;
                }
                let path = fields[fields.len() - 1].to_string();
                (path, Status::Unmerged)
            }
            _ => continue, // ignored ("!") and headers ("#")
        };
        if path.is_empty() {
            continue;
        }

        // "normal" mode reports a wholly-new directory as one entry ending
        // in '/' instead of listing its files — expand it so each file still
        // gets its own live row. Scoped to just this directory, so it stays
        // cheap even in a huge repo.
        if status == Status::Untracked && path.ends_with('/') {
            for file in untracked_files_in(root, &path)? {
                files.push(FileChange {
                    path: file,
                    status: Status::Untracked,
                    insertions: None,
                    deletions: None,
                });
            }
            continue;
        }

        let (insertions, deletions) = counts.get(&path).copied().unwrap_or((None, None));
        files.push(FileChange {
            path,
            status,
            insertions,
            deletions,
        });
    }
    Ok((branch, files))
}

fn untracked_files_in(root: &Path, dir: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .current_dir(root)
        .args([
            "--no-optional-locks",
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            dir,
        ])
        .output()
        .context("failed to run git ls-files")?;
    if !out.status.success() {
        anyhow::bail!("git ls-files failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn classify(xy: &str) -> Status {
    let mut chars = xy.chars();
    let staged = chars.next().unwrap_or('.');
    let worktree = chars.next().unwrap_or('.');
    if staged == 'A' {
        Status::Added
    } else if staged == 'D' || worktree == 'D' {
        Status::Deleted
    } else {
        Status::Modified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {:?} failed", args);
    }

    #[test]
    fn collect_reports_branch_name() {
        let dir = std::env::temp_dir().join(format!("changed_test_branch_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        git(&dir, &["init", "-q", "-b", "trunk"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join("tracked.rs"), "a\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "init"]);

        let snap = collect(&dir).unwrap();
        assert_eq!(snap.branch.as_deref(), Some("trunk"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_ignored_lists_force_added_files() {
        let dir = std::env::temp_dir().join(format!("changed_test_ti_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join(".gitignore"), "vendor/\n").unwrap();
        fs::create_dir_all(dir.join("vendor")).unwrap();
        fs::write(dir.join("vendor/checked_in.rs"), "a\n").unwrap();
        git(&dir, &["add", ".gitignore"]);
        git(&dir, &["add", "-f", "vendor/checked_in.rs"]);
        git(&dir, &["commit", "-qm", "init"]);

        let tracked = tracked_ignored(&dir).unwrap();
        assert!(tracked.contains(&PathBuf::from("vendor/checked_in.rs")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_reports_modified_added_untracked() {
        let dir = std::env::temp_dir().join(format!("changed_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join("tracked.rs"), "a\nb\nc\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "init"]);

        fs::write(dir.join("tracked.rs"), "a\nb\nc\nd\ne\n").unwrap();
        fs::write(dir.join("added.txt"), "hi\n").unwrap();

        let snap = collect(&dir).unwrap();
        let tracked = snap.files.iter().find(|f| f.path == "tracked.rs").unwrap();
        assert_eq!(tracked.status, Status::Modified);
        assert_eq!(tracked.insertions, Some(2));

        let added = snap.files.iter().find(|f| f.path == "added.txt").unwrap();
        assert_eq!(added.status, Status::Untracked);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_expands_wholly_new_directory_into_per_file_rows() {
        let dir = std::env::temp_dir().join(format!("changed_test_dir_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join("tracked.rs"), "a\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "init"]);

        // A brand-new, entirely untracked directory — the case where
        // `--untracked-files=normal` would otherwise collapse to one
        // `newdir/` row instead of one row per file.
        fs::create_dir_all(dir.join("newdir")).unwrap();
        fs::write(dir.join("newdir/a.txt"), "a\n").unwrap();
        fs::write(dir.join("newdir/b.txt"), "b\n").unwrap();

        let snap = collect(&dir).unwrap();
        assert!(
            snap.files.iter().all(|f| !f.path.ends_with('/')),
            "directory row should have been expanded: {:?}",
            snap.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
        let a = snap
            .files
            .iter()
            .find(|f| f.path == "newdir/a.txt")
            .unwrap();
        assert_eq!(a.status, Status::Untracked);
        let b = snap
            .files
            .iter()
            .find(|f| f.path == "newdir/b.txt")
            .unwrap();
        assert_eq!(b.status, Status::Untracked);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_reports_unmerged_files_during_rebase() {
        let dir = std::env::temp_dir().join(format!("changed_test_rebase_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join("f.txt"), "base\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "base"]);

        git(&dir, &["checkout", "-q", "-b", "feature"]);
        fs::write(dir.join("f.txt"), "base\nfeature\n").unwrap();
        git(&dir, &["commit", "-am", "feat"]);

        git(&dir, &["checkout", "-q", "main"]);
        fs::write(dir.join("f.txt"), "base\nmain\n").unwrap();
        git(&dir, &["commit", "-am", "main"]);

        git(&dir, &["checkout", "-q", "feature"]);
        let rebase = Command::new("git")
            .current_dir(&dir)
            .args(["rebase", "main"])
            .output()
            .unwrap();
        assert!(!rebase.status.success(), "rebase should stop on conflict");

        let snap = collect(&dir).unwrap();
        assert_eq!(snap.branch.as_deref(), Some("(detached)"));
        let conflict = snap.files.iter().find(|f| f.path == "f.txt").unwrap();
        assert_eq!(conflict.status, Status::Unmerged);

        let _ = fs::remove_dir_all(&dir);
    }
}
