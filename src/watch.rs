use anyhow::Result;
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

pub struct Watcher {
    pub rx: Receiver<Event>,
    _watcher: RecommendedWatcher,
}

pub fn spawn(root: &Path, git_dir: &Path) -> Result<Watcher> {
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(ev) = res {
            let _ = tx.send(ev);
        }
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;
    if should_watch_git_dir(root, git_dir) {
        watcher.watch(git_dir, RecursiveMode::Recursive)?;
    }
    Ok(Watcher {
        rx,
        _watcher: watcher,
    })
}

/// Linked worktrees keep `index`/`HEAD`/`refs` outside the checkout tree.
fn should_watch_git_dir(root: &Path, git_dir: &Path) -> bool {
    match (root.canonicalize(), git_dir.canonicalize()) {
        (Ok(root), Ok(git_dir)) => !git_dir.starts_with(&root),
        _ => true,
    }
}

/// Repo-aware ignore matcher used to decide whether a filesystem event can
/// change `git status` output. Root `.gitignore` and `.git/info/exclude` are
/// loaded up front; nested `.gitignore` files are loaded lazily and cached.
/// Global excludes (`core.excludesFile`) are not consulted — a miss there
/// only causes a redundant refresh, never a dropped one.
pub struct IgnoreSet {
    root: PathBuf,
    root_matcher: Option<Gitignore>,
    nested: HashMap<PathBuf, Option<Gitignore>>,
    /// Tracked files that match ignore rules (e.g. force-added). Ignore rules
    /// don't apply to tracked files, so edits to these must bypass the filter.
    tracked: HashSet<PathBuf>,
}

impl IgnoreSet {
    pub fn new(root: PathBuf, tracked: HashSet<PathBuf>) -> Self {
        let mut builder = GitignoreBuilder::new(&root);
        // Lower precedence first: excludes file, then the root .gitignore.
        let exclude = root.join(".git/info/exclude");
        if exclude.is_file() {
            builder.add(exclude);
        }
        let gitignore = root.join(".gitignore");
        if gitignore.is_file() {
            builder.add(gitignore);
        }
        IgnoreSet {
            root,
            root_matcher: builder.build().ok(),
            nested: HashMap::new(),
            tracked,
        }
    }

    fn is_tracked(&self, rel: &Path) -> bool {
        self.tracked.contains(rel)
    }

    /// Mirrors git semantics: an ignored parent directory hides its whole
    /// subtree, and among the rules that apply, the deepest `.gitignore`
    /// decides first.
    pub fn is_ignored(&mut self, rel: &Path, is_dir: bool) -> bool {
        let mut ancestors: Vec<PathBuf> = rel
            .ancestors()
            .skip(1)
            .filter(|a| !a.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .collect();
        ancestors.reverse();
        for dir in ancestors {
            if self.decides_ignored(&dir, true) {
                return true;
            }
        }
        self.decides_ignored(rel, is_dir)
    }

    fn decides_ignored(&mut self, rel: &Path, is_dir: bool) -> bool {
        let mut dir = rel.parent();
        while let Some(d) = dir {
            if d.as_os_str().is_empty() {
                break;
            }
            let abs = self.root.join(d);
            if let Some(matcher) = self.matcher_for(&abs) {
                let p = rel.strip_prefix(d).unwrap_or(rel);
                match matcher.matched(p, is_dir) {
                    Match::Ignore(_) => return true,
                    Match::Whitelist(_) => return false,
                    Match::None => {}
                }
            }
            dir = d.parent();
        }
        if let Some(matcher) = &self.root_matcher {
            match matcher.matched(rel, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }

    fn matcher_for(&mut self, dir_abs: &Path) -> Option<&Gitignore> {
        self.nested
            .entry(dir_abs.to_path_buf())
            .or_insert_with(|| {
                let gitignore = dir_abs.join(".gitignore");
                if !gitignore.is_file() {
                    return None;
                }
                let mut builder = GitignoreBuilder::new(dir_abs);
                builder.add(gitignore);
                builder.build().ok()
            })
            .as_ref()
    }
}

/// True when this event can change what `git status` reports.
pub fn is_interesting(ev: &Event, root: &Path, git_dir: &Path, ignores: &mut IgnoreSet) -> bool {
    match ev.kind {
        // Pure metadata churn (atime, chmod) never affects the worktree diff.
        EventKind::Access(_) | EventKind::Other | EventKind::Modify(ModifyKind::Metadata(_)) => {
            return false;
        }
        _ => {}
    }
    ev.paths
        .iter()
        .any(|p| path_is_interesting(p, root, git_dir, ignores))
}

fn path_is_interesting(p: &Path, root: &Path, git_dir: &Path, ignores: &mut IgnoreSet) -> bool {
    if let Ok(rel) = p.strip_prefix(git_dir) {
        return is_git_metadata(rel);
    }
    // Unknown provenance — refresh conservatively rather than miss a change.
    let Ok(rel) = p.strip_prefix(root) else {
        return true;
    };
    if rel.as_os_str().is_empty() {
        return false;
    }
    if rel.starts_with(".git") {
        return is_git_metadata(rel.strip_prefix(".git").unwrap_or(rel));
    }
    if ignores.is_tracked(rel) {
        return true;
    }
    // Ignore-rule edits can change status output without touching other files.
    if rel.file_name() == Some(OsStr::new(".gitignore")) {
        return true;
    }
    !ignores.is_ignored(rel, p.is_dir())
}

/// Git-internal paths that can change status output — commits, checkouts,
/// staging. Object store churn, gc and lock files are skipped.
fn is_git_metadata(rel: &Path) -> bool {
    if rel.as_os_str().is_empty() {
        return false;
    }
    rel == Path::new("index")
        || rel == Path::new("HEAD")
        || rel == Path::new("packed-refs")
        || rel == Path::new("ORIG_HEAD")
        || rel.starts_with("refs")
        || rel
            .file_name()
            .is_some_and(|f| f.to_string_lossy().ends_with("_HEAD"))
        || rel.starts_with("rebase-merge")
        || rel.starts_with("rebase-apply")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("changed_watch_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ignored_paths_and_their_subtrees_are_filtered() {
        let dir = temp_dir("ign");
        fs::write(dir.join(".gitignore"), "target/\n*.log\n").unwrap();
        let mut ignores = IgnoreSet::new(dir.clone(), HashSet::new());

        assert!(ignores.is_ignored(Path::new("target"), true));
        assert!(ignores.is_ignored(Path::new("target/debug/build.rs"), false));
        assert!(ignores.is_ignored(Path::new("foo.log"), false));
        assert!(!ignores.is_ignored(Path::new("src/main.rs"), false));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_gitignore_applies_and_overrides_root() {
        let dir = temp_dir("nested");
        fs::write(dir.join(".gitignore"), "*.tmp\n").unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/.gitignore"), "!keep.tmp\n").unwrap();
        let mut ignores = IgnoreSet::new(dir.clone(), HashSet::new());

        assert!(ignores.is_ignored(Path::new("sub/drop.tmp"), false));
        assert!(!ignores.is_ignored(Path::new("sub/keep.tmp"), false));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_but_ignored_files_bypass_the_filter() {
        let dir = temp_dir("tracked");
        fs::write(dir.join(".gitignore"), "vendor/\n").unwrap();
        let tracked: HashSet<PathBuf> = ["vendor/gen/checked_in.rs"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let mut ignores = IgnoreSet::new(dir.clone(), tracked);

        let git_dir = dir.join(".git");
        let ev = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(dir.join("vendor/gen/checked_in.rs"));
        assert!(is_interesting(&ev, &dir, &git_dir, &mut ignores));
        let ev = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(dir.join("vendor/other/output.rs"));
        assert!(!is_interesting(&ev, &dir, &git_dir, &mut ignores));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_internal_paths_scoped_to_status_relevant_state() {
        assert!(is_git_metadata(Path::new("index")));
        assert!(is_git_metadata(Path::new("HEAD")));
        assert!(is_git_metadata(Path::new("packed-refs")));
        assert!(is_git_metadata(Path::new("refs/heads/main")));
        assert!(is_git_metadata(Path::new("MERGE_HEAD")));
        assert!(!is_git_metadata(Path::new("objects/ab/cdef")));
        assert!(!is_git_metadata(Path::new("index.lock")));
        assert!(!is_git_metadata(Path::new("logs/HEAD")));
        assert!(is_git_metadata(Path::new("rebase-merge/git-rebase-todo")));
        assert!(!is_git_metadata(Path::new("COMMIT_EDITMSG")));
    }

    #[test]
    fn external_gitdir_index_updates_are_interesting() {
        let worktree = temp_dir("wt");
        let git_dir = temp_dir("gitdir");
        fs::create_dir_all(worktree.join("src")).unwrap();
        let mut ignores = IgnoreSet::new(worktree.clone(), HashSet::new());

        let ev = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(git_dir.join("index"));
        assert!(is_interesting(&ev, &worktree, &git_dir, &mut ignores));

        let ev = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(git_dir.join("objects/pack/tmp_pack"));
        assert!(!is_interesting(&ev, &worktree, &git_dir, &mut ignores));

        let _ = fs::remove_dir_all(&worktree);
        let _ = fs::remove_dir_all(&git_dir);
    }

    #[test]
    fn metadata_only_events_are_not_interesting() {
        let dir = temp_dir("meta");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let mut ignores = IgnoreSet::new(dir.clone(), HashSet::new());

        let git_dir = dir.join(".git");
        use notify::event::{DataChange, MetadataKind};
        let metadata = Event::new(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)))
            .add_path(dir.join("src/main.rs"));
        assert!(!is_interesting(&metadata, &dir, &git_dir, &mut ignores));

        let data = Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Any)))
            .add_path(dir.join("src/main.rs"));
        assert!(is_interesting(&data, &dir, &git_dir, &mut ignores));

        let _ = fs::remove_dir_all(&dir);
    }
}
