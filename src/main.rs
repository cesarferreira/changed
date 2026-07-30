mod app;
mod git;
mod ui;
mod watch;

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{execute, terminal};
use std::io::stdout;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "changed",
    version,
    about = "Live read-only TUI showing what changed in your git worktree"
)]
struct Cli {}

/// Quiet period after the last interesting filesystem event before
/// re-querying git.
const DEBOUNCE: Duration = Duration::from_millis(120);
/// Minimum gap between git refreshes. Sustained churn (builds, file sync,
/// formatters) collapses to at most one `git status` per interval instead of
/// one per debounced burst.
const MIN_COLLECT_INTERVAL: Duration = Duration::from_millis(750);

/// Safety net when git state changes without a worktree filesystem event (rare
/// tooling paths). Kept long so large repos aren't polled constantly.
const POLL_INTERVAL: Duration = Duration::from_secs(30);

fn main() -> Result<()> {
    let _ = Cli::parse();

    let root = git::repo_root()?;
    let git_dir = git::git_dir(&root)?;
    let watcher = watch::spawn(&root, &git_dir)?;
    let tracked = git::tracked_ignored(&root).unwrap_or_default();
    let mut ignores = watch::IgnoreSet::new(root.clone(), tracked);

    let mut app = App::new(root.clone());
    app.apply(git::collect(&root)?, Instant::now());

    let mut terminal = setup_terminal()?;
    let res = run(
        &mut terminal,
        &mut app,
        &root,
        &git_dir,
        &watcher,
        &mut ignores,
    );
    restore_terminal()?;
    res
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    root: &std::path::Path,
    git_dir: &std::path::Path,
    watcher: &watch::Watcher,
    ignores: &mut watch::IgnoreSet,
) -> Result<()> {
    let mut dirty = true;
    let mut pending_at: Option<Instant> = None;
    let mut last_collect = Instant::now();
    let mut drawn_ago_bucket: Option<u64> = None;

    loop {
        let now = Instant::now();

        if now.duration_since(last_collect) >= POLL_INTERVAL {
            pending_at = Some(now);
        }

        if watcher
            .rx
            .try_iter()
            .any(|ev| watch::is_interesting(&ev, root, git_dir, ignores))
        {
            pending_at = Some(now + DEBOUNCE);
        }

        if let Some(at) = pending_at {
            if now >= at && now.duration_since(last_collect) >= MIN_COLLECT_INTERVAL {
                if let Ok(snap) = git::collect(root) {
                    if app.apply(snap, now) {
                        dirty = true;
                    }
                }
                last_collect = now;
                pending_at = None;
            }
        }

        let next_collect = pending_at.map(|at| {
            if now >= at {
                last_collect + MIN_COLLECT_INTERVAL
            } else {
                at
            }
        });
        if event::poll(tick_duration(app, now, next_collect))? {
            match event::read()? {
                Event::Key(key)
                    if key.kind == KeyEventKind::Press && is_quit(key.code, key.modifiers) =>
                {
                    return Ok(());
                }
                Event::Resize(..) => dirty = true,
                _ => {}
            }
        }

        // Redraw only when something visible can differ: new snapshot, an
        // animating flash, or the "last change Ns ago" footer ticking over.
        let now = Instant::now();
        let ago_bucket = app
            .last_change
            .filter(|_| !app.is_clean())
            .map(|t| now.duration_since(t).as_secs());
        if dirty || app.has_active_flash(now) || ago_bucket != drawn_ago_bucket {
            terminal.draw(|f| ui::draw(f, app, now))?;
            dirty = false;
            drawn_ago_bucket = ago_bucket;
        }
    }
}

/// How long `event::poll` may block: short while a flash animates or a
/// refresh is due, and just past the next one-second boundary while the
/// "ago" footer is visible so it updates on time.
fn tick_duration(app: &App, now: Instant, next_collect: Option<Instant>) -> Duration {
    if app.has_active_flash(now) {
        return Duration::from_millis(50);
    }
    let mut tick = Duration::from_millis(500);
    if let Some(at) = next_collect {
        tick = tick.min(at.saturating_duration_since(now));
    }
    if let Some(t) = app.last_change.filter(|_| !app.is_clean()) {
        let elapsed = now.duration_since(t);
        let until_next_second = Duration::from_secs(1)
            .saturating_sub(Duration::from_millis(elapsed.subsec_millis() as u64));
        tick = tick.min(until_next_second);
    }
    tick.max(Duration::from_millis(10))
}

fn is_quit(code: KeyCode, mods: KeyModifiers) -> bool {
    matches!(code, KeyCode::Char('q') | KeyCode::Esc)
        || (mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c'))
}

fn setup_terminal() -> Result<ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>>
{
    terminal::enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen)?;
    std::panic::set_hook(Box::new(|info| {
        let _ = restore_terminal();
        eprintln!("{info}");
    }));
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    Ok(ratatui::Terminal::new(backend)?)
}

fn restore_terminal() -> Result<()> {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(stdout(), terminal::LeaveAlternateScreen);
    Ok(())
}
