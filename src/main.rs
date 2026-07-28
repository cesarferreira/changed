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

fn main() -> Result<()> {
    let _ = Cli::parse();

    let root = git::repo_root()?;
    let watcher = watch::spawn(&root)?;

    let mut app = App::new();
    app.apply(git::collect(&root)?, Instant::now());

    let mut terminal = setup_terminal()?;
    let res = run(&mut terminal, &mut app, &root, &watcher);
    restore_terminal()?;
    res
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    root: &std::path::Path,
    watcher: &watch::Watcher,
) -> Result<()> {
    let tick = Duration::from_millis(250);
    let mut last_draw = Instant::now();
    terminal.draw(|f| ui::draw(f, app, Instant::now()))?;

    loop {
        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && is_quit(key.code, key.modifiers) {
                    return Ok(());
                }
            }
        }

        let now = Instant::now();
        let fs_changed = watcher.rx.try_iter().count() > 0;

        let mut redraw = false;
        if fs_changed {
            if let Ok(snap) = git::collect(root) {
                redraw |= app.apply(snap, now);
            }
        }
        // Redraw at least every tick so relative time and flashes stay live.
        if now.duration_since(last_draw) >= tick {
            redraw = true;
        }

        if redraw {
            terminal.draw(|f| ui::draw(f, app, Instant::now()))?;
            last_draw = Instant::now();
        }
    }
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
