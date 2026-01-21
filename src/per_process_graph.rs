//! Per-process graphing module
// This module will provide a UI tab to display CPU and memory usage graphs for a selected process over time.

use ratatui::{Frame, layout::Rect, widgets::{Block, Borders, Dataset, GraphType, Chart, Paragraph, Table, Row, Cell}, style::{Style, Modifier, Color}, layout::{Layout, Constraint, Direction, Alignment}, text::{Line, Span}};
use crate::process::ProcessManager;
use crate::graph::GraphData;

const PROCESS_TABLE_HEIGHT: usize = 12;

/// Render the per-process graph tab.
pub fn render_per_process_graph_tab(
    frame: &mut Frame, 
    area: Rect, 
    process_manager: &ProcessManager,
    graph_data: &GraphData,
    selected_process_index: usize,
    scroll_offset: usize,
    selected_process_for_graph: Option<u32>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(0),     // Content
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Per-Process Graph View")
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    if let Some(pid) = selected_process_for_graph {
        // Show graphs for selected process
        let graph_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),  // CPU Graph
                Constraint::Percentage(50),  // Memory Graph
            ])
            .split(chunks[1]);

        // Get process info
        if let Some(process) = process_manager.get_processes().iter().find(|p| p.pid == pid) {
            // Get history data
            if let Some((cpu_history, mem_history)) = graph_data.get_process_history(pid) {
                // CPU Graph
                let cpu_data: Vec<(f64, f64)> = cpu_history.iter()
                    .enumerate()
                    .map(|(i, &usage)| (i as f64, usage as f64))
                    .collect();

                let cpu_dataset = Dataset::default()
                    .name("CPU Usage")
                    .marker(ratatui::symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&cpu_data);

                let cpu_chart = Chart::new(vec![cpu_dataset])
                    .block(Block::default()
                        .title(format!("CPU Usage for {} (PID: {})", process.name, pid))
                        .borders(Borders::ALL))
                    .x_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, cpu_history.len() as f64])
                        .labels(vec![]))
                    .y_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, 100.0])
                        .labels(vec!["0%".into(), "50%".into(), "100%".into()]));

                frame.render_widget(cpu_chart, graph_chunks[0]);

                // Memory Graph
                let memory_data: Vec<(f64, f64)> = mem_history.iter()
                    .enumerate()
                    .map(|(i, &usage)| (i as f64, usage as f64 / (1024.0 * 1024.0)))
                    .collect();

                let max_memory = memory_data.iter()
                    .map(|&(_, y)| y)
                    .fold(0.0, f64::max)
                    .max(1.0);  // Ensure we have at least 1MB range

                let memory_dataset = Dataset::default()
                    .name("Memory Usage")
                    .marker(ratatui::symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Green))
                    .data(&memory_data);

                let memory_chart = Chart::new(vec![memory_dataset])
                    .block(Block::default()
                        .title(format!("Memory Usage for {} (PID: {})", process.name, pid))
                        .borders(Borders::ALL))
                    .x_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, mem_history.len() as f64])
                        .labels(vec![]))
                    .y_axis(ratatui::widgets::Axis::default()
                        .bounds([0.0, max_memory * 1.2])
                        .labels(vec![
                            "0 MB".into(),
                            format!("{:.1} MB", max_memory / 2.0).into(),
                            format!("{:.1} MB", max_memory).into(),
                        ]));

                frame.render_widget(memory_chart, graph_chunks[1]);
            }
        }
    } else {
        // Show process selection list
        let processes = process_manager.get_processes();
        let headers = ["PID", "NAME", "CPU%", "MEM(MB)", "USER"];
        
        let header_cells = headers
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
        
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::Blue))
            .height(1);

        let rows: Vec<Row> = processes
            .iter()
            .skip(scroll_offset)
            .take(PROCESS_TABLE_HEIGHT - 2)
            .enumerate()
            .map(|(i, process)| {
                let idx = scroll_offset + i;
                let highlight = idx == selected_process_index;
                let style = if highlight {
                    Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if i % 2 == 0 {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Blue)
                };

                let memory_mb = process.memory_usage / (1024 * 1024);

                Row::new(vec![
                    Cell::from(process.pid.to_string()).style(style),
                    Cell::from(process.name.clone()).style(Style::default().fg(Color::Green)),
                    Cell::from(format!("{:.1}%", process.cpu_usage)).style(style),
                    Cell::from(format!("{}", memory_mb)).style(style),
                    Cell::from(process.user.clone().unwrap_or_default()).style(Style::default().fg(Color::Magenta)),
                ])
            })
            .collect();

        let table = Table::new(rows)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Select a Process (↑↓ to move, Enter to select, Esc to return)"))
            .widths(&[
                Constraint::Length(8),   // PID
                Constraint::Length(20),  // NAME
                Constraint::Length(8),   // CPU%
                Constraint::Length(10),  // MEM(MB)
                Constraint::Length(12),  // USER
            ]);

        frame.render_widget(table, chunks[1]);
    }
} 