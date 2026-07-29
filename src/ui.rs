use crate::app::App;
use crate::git::Status;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

pub fn draw(f: &mut Frame, app: &App, now: Instant) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Length(1), // rule
        Constraint::Length(1), // summary
        Constraint::Length(1), // rule
        Constraint::Min(1),    // file list
        Constraint::Length(1), // rule
        Constraint::Length(1), // footer
    ])
    .split(area);

    let width = area.width as usize;
    let dim = Style::default().fg(Color::DarkGray);
    let rule_style = Style::default().fg(Color::Blue);

    // Title: repo path (+ subtle branch on the right)
    let path_label = compress(&home_abbrev(&app.root), width.saturating_sub(2));
    let mut title = vec![
        Span::styled("● ", Style::default().fg(Color::LightMagenta)),
        Span::styled(
            path_label.clone(),
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(branch) = &app.branch {
        let label = format!(" {branch}");
        let used = 2 + path_label.chars().count() + label.chars().count();
        if width > used + 1 {
            title.push(Span::raw(" ".repeat(width - used)));
            title.push(Span::styled("", Style::default().fg(Color::Blue)));
            title.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(title)), chunks[0]);
    f.render_widget(rule(width, rule_style), chunks[1]);

    // Summary
    f.render_widget(Paragraph::new(summary_line(app, width)), chunks[2]);
    f.render_widget(rule(width, rule_style), chunks[3]);

    // File list (or centered clean message)
    if app.is_clean() {
        let clean = Paragraph::new(Line::from(Span::styled(
            "✓ working tree clean",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center);
        let centered = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(chunks[4]);
        f.render_widget(clean, centered[1]);
    } else {
        let capacity = chunks[4].height as usize;
        let (ins_width, del_width) = stat_column_widths(&app.rows);
        let name_width = file_name_width(width, ins_width, del_width);
        let lines: Vec<Line> = app
            .rows
            .iter()
            .take(capacity)
            .map(|row| file_line(row, now, name_width, ins_width, del_width))
            .collect();
        f.render_widget(Paragraph::new(lines), chunks[4]);
    }

    f.render_widget(rule(width, rule_style), chunks[5]);

    // Footer
    if let Some(t) = app.last_change.filter(|_| !app.is_clean()) {
        let footer = Line::from(vec![
            Span::styled("last change  ", dim),
            Span::styled(
                ago(now.duration_since(t)),
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        f.render_widget(Paragraph::new(footer), chunks[6]);
    }
}

fn rule(width: usize, style: Style) -> Paragraph<'static> {
    Paragraph::new(Span::styled("─".repeat(width), style))
}

/// Status counts on the left; total lines changed pushed flush right
/// (mirroring the branch name in the title row).
fn summary_line(app: &App, width: usize) -> Line<'static> {
    let mut spans = Vec::new();
    let push = |sym: &str, n: usize, label: &str, color: Color, spans: &mut Vec<Span<'static>>| {
        if n == 0 {
            return;
        }
        if !spans.is_empty() {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(
            format!("{sym} {n} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(label.to_string(), Style::default().fg(color)));
    };
    push(
        "●",
        app.count(Status::Modified),
        "modified",
        Color::LightYellow,
        &mut spans,
    );
    push(
        "+",
        app.count(Status::Added),
        "added",
        Color::LightGreen,
        &mut spans,
    );
    push(
        "-",
        app.count(Status::Deleted),
        "deleted",
        Color::LightRed,
        &mut spans,
    );
    push(
        "»",
        app.count(Status::Renamed),
        "renamed",
        Color::LightCyan,
        &mut spans,
    );
    push(
        "?",
        app.count(Status::Untracked),
        "untracked",
        Color::LightMagenta,
        &mut spans,
    );

    let mut right = Vec::new();
    let insertions = app.total_insertions();
    let deletions = app.total_deletions();
    if insertions > 0 {
        right.push(Span::styled(
            format!("+{insertions} "),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if deletions > 0 {
        right.push(Span::styled(
            format!("-{deletions}"),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if !right.is_empty() {
        let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let right_len: usize = right.iter().map(|s| s.content.chars().count()).sum();
        let used = left_len + right_len;
        let gap = if width > used { width - used } else { 2 };
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(right);
    }

    Line::from(spans)
}

/// Replace the user's home directory prefix with `~`, matching shell convention.
fn home_abbrev(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() => full
            .strip_prefix(&home)
            .map(|rest| format!("~{rest}"))
            .unwrap_or(full),
        _ => full,
    }
}

/// Widest `+N`/`-N` labels across all rows, so every row's insertions and
/// deletions column lines up in a fixed-width field instead of drifting with
/// digit count.
fn stat_column_widths(rows: &[crate::app::Row]) -> (usize, usize) {
    let mut ins_width = 0;
    let mut del_width = 0;
    for row in rows {
        if let Some(n) = row.file.insertions.filter(|n| *n > 0) {
            ins_width = ins_width.max(format!("+{n}").len());
        }
        if let Some(n) = row.file.deletions.filter(|n| *n > 0) {
            del_width = del_width.max(format!("-{n}").len());
        }
    }
    (ins_width, del_width)
}

fn file_line(
    row: &crate::app::Row,
    now: Instant,
    name_width: usize,
    ins_width: usize,
    del_width: usize,
) -> Line<'static> {
    let flash = row.flash_strength(now);
    let color = match row.file.status {
        Status::Modified => Color::LightYellow,
        Status::Added => Color::LightGreen,
        Status::Deleted => Color::LightRed,
        Status::Renamed => Color::LightCyan,
        Status::Untracked => Color::LightMagenta,
    };

    let with_flash = |style: Style| apply_flash_bg(style, flash);

    let name = compress(&row.file.path, name_width);
    let padded = format!("{name:<name_width$}");

    let mut spans = vec![
        Span::styled(
            format!("{} ", row.file.status.symbol()),
            with_flash(Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ),
        Span::styled(padded, with_flash(Style::default().fg(Color::White))),
    ];

    if ins_width > 0 {
        let text = row
            .file
            .insertions
            .filter(|n| *n > 0)
            .map(|n| format!("+{n}"))
            .unwrap_or_default();
        spans.push(Span::styled(
            format!("  {text:>ins_width$}"),
            with_flash(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    if del_width > 0 {
        let text = row
            .file
            .deletions
            .filter(|n| *n > 0)
            .map(|n| format!("-{n}"))
            .unwrap_or_default();
        spans.push(Span::styled(
            format!("  {text:>del_width$}"),
            with_flash(
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    Line::from(spans)
}

/// Green background that fades from bright to none as `strength` goes 1 → 0.
fn apply_flash_bg(mut style: Style, strength: f32) -> Style {
    if strength <= 0.0 {
        return style;
    }
    // Fades all the way to 0 (no floor) so the handoff to the real
    // background/foreground colors is imperceptible instead of a hard snap.
    let g = (220.0 * strength).round() as u8;
    if g == 0 {
        // Let the terminal's real background show through rather than
        // painting pure black over it (which could differ from the theme).
        return style;
    }
    style.bg = Some(Color::Rgb(0, g, 0));
    style
}

/// Name column takes exactly what's left after the status symbol and the
/// insertions/deletions columns, so `+N`/`-N` always land flush against the
/// right edge — lined up with the totals in the summary row above.
fn file_name_width(term_width: usize, ins_width: usize, del_width: usize) -> usize {
    let stats_width = (if ins_width > 0 { 2 + ins_width } else { 0 })
        + (if del_width > 0 { 2 + del_width } else { 0 });
    term_width.saturating_sub(2 + stats_width).max(4)
}

/// Compress `src/a/b/c/file.rs` → `src/.../file.rs` to fit within `max`.
fn compress(path: &str, max: usize) -> String {
    if path.chars().count() <= max {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        // No middle to drop — truncate from the front.
        let file: String = path.chars().collect();
        let keep = max.saturating_sub(1);
        let start = file.chars().count().saturating_sub(keep);
        return format!("…{}", file.chars().skip(start).collect::<String>());
    }
    let first = parts[0];
    let last = parts[parts.len() - 1];
    let candidate = format!("{first}/…/{last}");
    if candidate.chars().count() <= max {
        candidate
    } else {
        let keep = max.saturating_sub(1);
        let start = last.chars().count().saturating_sub(keep);
        format!("…{}", last.chars().skip(start).collect::<String>())
    }
}

fn ago(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s < 1 {
        "just now".to_string()
    } else if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else {
        format!("{}h ago", s / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Row};
    use crate::git::{FileChange, Status};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_summary_and_files() {
        let mut app = App::new(std::path::PathBuf::from("/repo"));
        app.branch = Some("main".into());
        app.last_change = Some(Instant::now());
        app.rows = vec![Row {
            file: FileChange {
                path: "src/git/status.rs".into(),
                status: Status::Modified,
                insertions: Some(24),
                deletions: Some(6),
            },
            changed_at: Instant::now(),
        }];

        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        let now = Instant::now();
        term.draw(|f| draw(f, &app, now)).unwrap();
        let text = buffer_text(term.backend());
        assert!(text.contains("/repo"), "title path missing");
        assert!(text.contains("main"), "branch missing");
        assert!(text.contains("status.rs"), "file missing:\n{text}");
        assert!(text.contains("+24"), "insertions missing:\n{text}");
    }

    fn buffer_text(backend: &TestBackend) -> String {
        let buf = backend.buffer();
        let area = *buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn compress_keeps_first_and_last() {
        let got = compress("src/some/really/long/path/to/file.rs", 20);
        assert!(got.starts_with("src/"), "{got}");
        assert!(got.ends_with("file.rs"), "{got}");
        assert!(got.chars().count() <= 20, "{got}");
    }

    #[test]
    fn clean_state_shows_message_once_centered() {
        let app = App::new(std::path::PathBuf::from("/repo"));
        let mut term = Terminal::new(TestBackend::new(40, 10)).unwrap();
        term.draw(|f| draw(f, &app, Instant::now())).unwrap();
        let text = buffer_text(term.backend());
        let count = text.matches("working tree clean").count();
        assert_eq!(count, 1, "expected once, got {count}:\n{text}");
    }

    #[test]
    fn flash_bg_fades_with_strength() {
        let full = apply_flash_bg(Style::default(), 1.0);
        let none = apply_flash_bg(Style::default(), 0.0);
        assert_eq!(full.bg, Some(Color::Rgb(0, 220, 0)));
        assert_eq!(none.bg, None);
    }

    #[test]
    fn flash_bg_has_no_floor() {
        // Near the tail of the fade there should be no lingering green plateau —
        // it should reach "no override" smoothly instead of snapping from a
        // visible shade straight to the terminal's real background.
        let almost_gone = apply_flash_bg(Style::default(), 0.001);
        assert_eq!(almost_gone.bg, None);
    }
}
