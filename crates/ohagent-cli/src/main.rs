//! ohAgent TUI — see what the agent is doing in real-time.
//!
//! Connects to ohAgent daemon via WebSocket, streams text and tool calls
//! just like Jcode TUI does, but through ohAgent's memory/skills/router.
//!
//! Usage:
//!   ohagent [--url ws://localhost:9090/v1/ws/chat]

use std::time::Instant;

use anyhow::Result;
use chrono::Local;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ── CLI args ──

#[derive(Parser, Debug)]
#[command(name = "ohagent", version, about = "ohAgent TUI client")]
struct Cli {
    /// WebSocket URL of ohAgent daemon
    #[arg(long, default_value = "ws://localhost:9090/v1/ws/chat")]
    url: String,

    /// Review mode: skip memory, skills, and past context for unbiased analysis
    #[arg(long, default_value_t = false)]
    review: bool,
}

// ── WebSocket protocol types ──

#[derive(Debug, Deserialize)]
struct WsEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    input: String,
    #[serde(default)]
    output: String,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    took_ms: u64,
    #[serde(default)]
    tokens_per_sec: u32,
    #[serde(default)]
    usage: Option<UsageInfo>,
    #[serde(default)]
    partial_content: String,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
}

// ── Chat history ──

#[derive(Debug, Clone)]
enum ChatEntry {
    UserMessage { text: String, timestamp: String },
    AgentToken { text: String },
    ToolCall { id: String, name: String, input: String },
    ToolResult { id: String, name: String, output: String, success: bool },
    Done { took_ms: u64, tokens: u32, tps: u32 },
    Error { message: String },
    Info { text: String },
}

struct ChatState {
    entries: Vec<ChatEntry>,
    scroll: usize,
    current_tool_input: Option<String>,
    current_tool_name: Option<String>,
    current_tool_id: Option<String>,
}

impl ChatState {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll: 0,
            current_tool_input: None,
            current_tool_name: None,
            current_tool_id: None,
        }
    }

    fn add(&mut self, entry: ChatEntry) {
        self.entries.push(entry);
        self.scroll = self.entries.len().saturating_sub(1);
    }

    fn visible_height(&self, area_height: usize) -> usize {
        area_height.saturating_sub(2) // borders
    }

    fn visible_entries(&self, area_height: usize) -> &[ChatEntry] {
        let h = self.visible_height(area_height);
        let start = self.scroll.saturating_sub(h.saturating_sub(1));
        let end = (start + h).min(self.entries.len());
        if start >= self.entries.len() {
            return &[];
        }
        &self.entries[start..end]
    }
}

// ── App ──

struct App {
    chat: ChatState,
    input: String,
    connected: bool,
    status_msg: String,
    model: String,
    start_time: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            chat: ChatState::new(),
            input: String::new(),
            connected: false,
            status_msg: "Connecting...".into(),
            model: "deepseek-chat".into(),
            start_time: Instant::now(),
        }
    }
}

// ── TUI ──

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup terminal
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_app(&mut terminal, &cli.url, cli.review).await;

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, ws_url: &str, review_mode: bool) -> Result<()> {
    let mut app = App::new();

    if review_mode {
        app.model.push_str(" [review]");
    }

    // Connect to daemon WebSocket using the raw URL string
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    app.connected = true;
    app.status_msg = "Connected".into();
    app.chat.add(ChatEntry::Info {
        text: format!("Connected to ohAgent daemon at {ws_url}"),
    });

    // Spawn reader task
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<WsEvent>();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(event) = serde_json::from_str::<WsEvent>(&text) {
                        let _ = event_tx.send(event);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let mut input_buffer = String::new();

    loop {
        // Draw
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),   // header
                    Constraint::Min(1),      // chat area
                    Constraint::Length(3),   // input
                ])
                .split(area);

            // ── Header ──
            let status_style = if app.connected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };
            let header = Paragraph::new(Text::from(Line::from(vec![
                Span::styled(" ohAgent ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("●", status_style),
                Span::raw(format!(" {} | uptime: {}s | model: {}",
                    app.status_msg,
                    app.start_time.elapsed().as_secs(),
                    app.model,
                )),
            ])))
            .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // ── Chat area ──
            let chat_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Chat ", Style::default().fg(Color::Cyan)));
            let _chat_area = chat_block.inner(chunks[1]);

            let mut lines: Vec<Line> = Vec::new();
            for entry in app.chat.visible_entries(chunks[1].height as usize) {
                match entry {
                    ChatEntry::UserMessage { text, .. } => {
                        lines.push(Line::from(vec![
                            Span::styled("▶ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                            Span::raw(text.clone()),
                        ]));
                        lines.push(Line::from(""));
                    }
                    ChatEntry::AgentToken { text } => {
                        // Collect consecutive tokens into the last line
                        if let Some(last) = lines.last_mut() {
                            last.push_span(Span::raw(text.clone()));
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled("🤖 ", Style::default().fg(Color::Cyan)),
                                Span::raw(text.clone()),
                            ]));
                        }
                    }
                    ChatEntry::ToolCall { name, input, .. } => {
                        let display_name = match name.as_str() {
                            "shell_exec" => "bash",
                            "file_read" => "read",
                            "file_write" => "write",
                            "file_edit" => "edit",
                            other => other,
                        };
                        let summary = input.lines().next().unwrap_or(input);
                        let truncated = if summary.len() > 80 {
                            format!("{}…", &summary[..80])
                        } else {
                            summary.to_string()
                        };
                        lines.push(Line::from(vec![
                            Span::styled("🔧 ", Style::default().fg(Color::Yellow)),
                            Span::styled(display_name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                            Span::raw(" "),
                            Span::styled(truncated, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    ChatEntry::ToolResult { output, success, .. } => {
                        let preview = output.lines()
                            .filter(|l| !l.is_empty())
                            .take(5)
                            .collect::<Vec<_>>()
                            .join("\n");
                        let icon = if *success { "✅" } else { "❌" };
                        if !preview.is_empty() {
                            let truncated = if preview.len() > 200 {
                                format!("{}…", &preview[..200])
                            } else {
                                preview
                            };
                            lines.push(Line::from(vec![
                                Span::raw(format!("{icon} ")),
                                Span::styled(truncated, Style::default().fg(Color::DarkGray)),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::raw(format!("{icon} (empty result)")),
                            ]));
                        }
                        lines.push(Line::from(""));
                    }
                    ChatEntry::Done { took_ms, tokens, tps } => {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("⚡ Done in {took_ms}ms · {tokens} tokens · {tps} tok/s"),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                        lines.push(Line::from(""));
                    }
                    ChatEntry::Error { message } => {
                        lines.push(Line::from(vec![
                            Span::styled("⚠ Error: ", Style::default().fg(Color::Red)),
                            Span::styled(message.clone(), Style::default().fg(Color::Red)),
                        ]));
                        lines.push(Line::from(""));
                    }
                    ChatEntry::Info { text } => {
                        lines.push(Line::from(vec![
                            Span::styled(text.clone(), Style::default().fg(Color::DarkGray)),
                        ]));
                        lines.push(Line::from(""));
                    }
                }
            }

            // waiting marker — unused for now

            let chat_widget = Paragraph::new(Text::from(lines))
                .block(chat_block)
                .wrap(Wrap { trim: false });
            f.render_widget(chat_widget, chunks[1]);

            // ── Input ──
            let input_block = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Message ", Style::default().fg(Color::Cyan)));
            let input_widget = Paragraph::new(input_buffer.as_str())
                .block(input_block)
                .style(Style::default());
            f.render_widget(input_widget, chunks[2]);
            // Show cursor at end of input
            let cx = chunks[2].x + 1 + input_buffer.len() as u16;
            let cy = chunks[2].y + 1;
            f.set_cursor_position(ratatui::prelude::Position::new(cx, cy));
        })?;

        // Handle events from WebSocket reader
        if let Ok(event) = event_rx.try_recv() {
            match event.event_type.as_str() {
                "started" => {
                    // First token about to arrive
                }
                "token" => {
                    app.chat.add(ChatEntry::AgentToken { text: event.content });
                    // Merge consecutive tokens into previous AgentToken
                    let len = app.chat.entries.len();
                    if len >= 2 {
                        if let ChatEntry::AgentToken { text: prev } = &app.chat.entries[len - 2] {
                            if let ChatEntry::AgentToken { text: last } = &app.chat.entries[len - 1] {
                                // Merge: replace both with one combined entry
                                let combined = format!("{prev}{last}");
                                app.chat.entries.pop();
                                app.chat.entries.pop();
                                app.chat.entries.push(ChatEntry::AgentToken { text: combined });
                            }
                        }
                    }
                }
                "tool_call_start" => {
                    app.chat.add(ChatEntry::ToolCall {
                        id: event.id.clone(),
                        name: event.name.clone(),
                        input: event.input.clone(),
                    });
                    app.status_msg = format!("🔧 {}", event.name);
                }
                "tool_result" => {
                    app.chat.add(ChatEntry::ToolResult {
                        id: event.id,
                        name: event.name,
                        output: event.output,
                        success: event.success,
                    });
                    app.status_msg = "Connected".into();
                }
                "done" => {
                    app.chat.add(ChatEntry::Done {
                        took_ms: event.took_ms,
                        tokens: event.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
                        tps: event.tokens_per_sec,
                    });
                }
                "error" => {
                    app.chat.add(ChatEntry::Error { message: event.message });
                    app.status_msg = "Error".into();
                }
                "cancelled" => {
                    app.chat.add(ChatEntry::Info { text: "⚠ Cancelled".into() });
                    app.status_msg = "Connected".into();
                }
                _ => {}
            }
        }

        // Handle keyboard input
        if crossterm::event::poll(std::time::Duration::from_millis(50))? {
            match crossterm::event::read()? {
                crossterm::event::Event::Key(key) => {
                    match key.code {
                        crossterm::event::KeyCode::Enter => {
                            let msg = input_buffer.trim().to_string();
                            if !msg.is_empty() {
                                let timestamp = Local::now().format("%H:%M:%S").to_string();

                                // Add to chat
                                app.chat.add(ChatEntry::UserMessage {
                                    text: msg.clone(),
                                    timestamp: timestamp.clone(),
                                });

                                // Send via WebSocket
                                let mut chat_msg = serde_json::json!({
                                    "type": "chat",
                                    "model": app.model,
                                    "messages": [{"role": "user", "content": msg}],
                                });
                                if review_mode {
                                    chat_msg["review"] = serde_json::json!(true);
                                }
                                if ws_tx.send(Message::Text(chat_msg.to_string().into())).await.is_err() {
                                    app.connected = false;
                                    app.status_msg = "Disconnected".into();
                                }

                                input_buffer.clear();
                            }
                        }
                        crossterm::event::KeyCode::Char(c) => {
                            // Ctrl+C or Ctrl+D to quit
                            if c == 'c' || c == 'd' {
                                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
                                    break;
                                }
                            }
                            input_buffer.push(c);
                        }
                        crossterm::event::KeyCode::Backspace => {
                            input_buffer.pop();
                        }
                        crossterm::event::KeyCode::Esc => {
                            // Send cancel
                            let _ = ws_tx.send(Message::Text(
                                serde_json::json!({"type": "cancel"}).to_string().into()
                            )).await;
                        }
                        crossterm::event::KeyCode::Up => {
                            app.chat.scroll = app.chat.scroll.saturating_sub(1);
                        }
                        crossterm::event::KeyCode::Down => {
                            app.chat.scroll = (app.chat.scroll + 1).min(app.chat.entries.len().saturating_sub(1));
                        }
                        crossterm::event::KeyCode::PageUp => {
                            app.chat.scroll = app.chat.scroll.saturating_sub(10);
                        }
                        crossterm::event::KeyCode::PageDown => {
                            app.chat.scroll = (app.chat.scroll + 10).min(app.chat.entries.len().saturating_sub(1));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}
