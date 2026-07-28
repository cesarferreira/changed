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

    // Title (+ subtle branch on the right)
    let mut title = vec![
        Span::styled("● ", Style::default().fg(Color::LightMagenta)),
        Span::styled(
            "changed",
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(branch) = &app.branch {
        let label = format!(" {branch}");
        let used = 2 + "changed".len() + label.len();
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
    f.render_widget(Paragraph::new(summary_line(app)), chunks[2]);
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
        let name_width = name_column_width(width);
        let lines: Vec<Line> = app
            .rows
            .iter()
            .take(capacity)
            .map(|row| file_line(row, now, name_width))
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

fn summary_line(app: &App) -> Line<'static> {
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
    Line::from(spans)
}

fn file_line(row: &crate::app::Row, now: Instant, name_width: usize) -> Line<'static> {
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

    if let Some(ins) = row.file.insertions.filter(|n| *n > 0) {
        spans.push(Span::styled(
            format!("  +{ins}"),
            with_flash(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    if let Some(del) = row.file.deletions.filter(|n| *n > 0) {
        spans.push(Span::styled(
            format!("  -{del}"),
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
    let g = (30.0 + 190.0 * strength).round() as u8;
    style.bg = Some(Color::Rgb(0, g, 0));
    style
}

fn name_column_width(term_width: usize) -> usize {
    // Leave room for status symbol + numstat columns.
    term_width.saturating_sub(2 + 14).clamp(12, 80)
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
        let mut app = App::new();
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
        assert!(text.contains("changed"), "title missing");
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
        let app = App::new();
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
}
