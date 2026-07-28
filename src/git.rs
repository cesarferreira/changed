use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

impl Status {
    pub fn symbol(self) -> char {
        match self {
            Status::Modified => 'M',
            Status::Added => 'A',
            Status::Deleted => 'D',
            Status::Renamed => 'R',
            Status::Untracked => '?',
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

pub fn collect(root: &Path) -> Result<Snapshot> {
    let branch = branch_name(root);
    let counts = numstat(root)?;
    let files = status(root, &counts)?;
    Ok(Snapshot { branch, files })
}

fn branch_name(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() { None } else { Some(name) }
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

fn status(root: &Path, counts: &NumStat) -> Result<Vec<FileChange>> {
    let out = Command::new("git")
        .current_dir(root)
        .args([
            // Skip the index refresh git status does by default — avoids
            // contending for .git/index.lock with a concurrent `git commit`
            // (this runs on every file-system change `changed` observes).
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
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

    let mut files = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
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
    Ok(files)
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
        let a = snap.files.iter().find(|f| f.path == "newdir/a.txt").unwrap();
        assert_eq!(a.status, Status::Untracked);
        let b = snap.files.iter().find(|f| f.path == "newdir/b.txt").unwrap();
        assert_eq!(b.status, Status::Untracked);

        let _ = fs::remove_dir_all(&dir);
    }
}
