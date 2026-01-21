//! Process logging module
// This module will provide a UI tab to display a table logging when processes have closed, their uptime, and related info.

use ratatui::{Frame, layout::Rect};
use chrono::{DateTime, Local};

/// Struct to store exited process info for the log.
#[derive(Clone)]
pub struct ProcessExitLogEntry {
    pub pid: u32,
    pub name: String,
    pub user: Option<String>,
    pub start_time: String,
    pub exit_time: DateTime<Local>,
    pub uptime_secs: u64,
}

/// Render the process log tab.
pub fn render_process_log_tab(frame: &mut Frame, area: Rect, log: &[ProcessExitLogEntry]) {
    use ratatui::widgets::{Table, Row, Cell, Block, Borders};
    use ratatui::style::{Style, Color};
    use ratatui::layout::Constraint;

    let header = Row::new(vec![
        Cell::from("PID").style(Style::default().fg(Color::Black)),
        Cell::from("Name").style(Style::default().fg(Color::Black)),
        Cell::from("User").style(Style::default().fg(Color::Black)),
        Cell::from("Start Time").style(Style::default().fg(Color::Black)),
        Cell::from("Exit Time").style(Style::default().fg(Color::Black)),
        Cell::from("Uptime").style(Style::default().fg(Color::Black)),
    ]);
    let rows: Vec<Row> = log.iter().rev().map(|entry| {
        Row::new(vec![
            Cell::from(entry.pid.to_string()),
            Cell::from(entry.name.clone()),
            Cell::from(entry.user.clone().unwrap_or_default()),
            Cell::from(entry.start_time.clone()),
            Cell::from(entry.exit_time.format("%Y-%m-%d %H:%M:%S").to_string()),
            Cell::from(format!("{}s", entry.uptime_secs)),
        ]).style(Style::default().fg(Color::Black))
    }).collect();
    let table = Table::new(rows)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Exited Processes Log").style(Style::default().fg(Color::Black)))
        .widths(&[
            Constraint::Length(8),
            Constraint::Length(20),
            Constraint::Length(12),
            Constraint::Length(19),
            Constraint::Length(19),
            Constraint::Length(8),
        ]);
    frame.render_widget(table, area);
} 