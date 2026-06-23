use std::io;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;
use va_client::endpoint::ServerEndpoint;
use va_client::events::ChatClientMessage;

mod app;
mod chat;
mod chat_socket;
mod config;
mod data;
mod detail;
mod render;
mod runtime_socket;
mod selection;
mod theme;
mod transport;

use app::{AppView, TuiApp};
use chat_socket::{run_chat_socket, ChatSocketEvent};
use config::{resolve_endpoint, Args, RuntimeEnv};
use data::{fetch_snapshot, DashboardSnapshot};
use runtime_socket::{run_runtime_sockets, RuntimeSocketEvent};
use transport::{HttpTransport, TuiError};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("va-tui: {error}");
        std::process::exit(2);
    }
}

async fn run() -> Result<(), TuiError> {
    let args = Args::parse();
    let endpoint = resolve_endpoint(&args, &RuntimeEnv::current())?;
    let transport = HttpTransport::new(endpoint.clone());
    if args.once {
        let snapshot = fetch_snapshot(&transport).await?;
        print_once(&endpoint, &snapshot);
        return Ok(());
    }
    run_dashboard(
        endpoint,
        transport,
        Duration::from_millis(args.refresh_ms.max(250)),
    )
    .await
}

async fn run_dashboard(
    endpoint: ServerEndpoint,
    transport: HttpTransport,
    _refresh: Duration,
) -> Result<(), TuiError> {
    let (mut terminal, _guard) = enter_terminal()?;
    let mut app = TuiApp::new(&endpoint);
    let (chat_tx, chat_rx) = mpsc::unbounded_channel::<ChatClientMessage>();
    let (socket_event_tx, mut socket_event_rx) = mpsc::unbounded_channel::<ChatSocketEvent>();
    let chat_task = tokio::spawn(run_chat_socket(endpoint.clone(), chat_rx, socket_event_tx));
    let (runtime_event_tx, mut runtime_event_rx) = mpsc::unbounded_channel::<RuntimeSocketEvent>();
    let runtime_task = tokio::spawn(run_runtime_sockets(endpoint.clone(), runtime_event_tx));

    loop {
        while let Ok(event) = socket_event_rx.try_recv() {
            app.apply_chat_socket_event(event);
        }
        while let Ok(event) = runtime_event_rx.try_recv() {
            app.apply_runtime_socket_event(event);
        }
        app.clear_expired_exit_confirmation();
        terminal
            .draw(|frame| render::render(frame, &app))
            .map_err(|source| TuiError::Io {
                action: "drawing terminal dashboard",
                source,
            })?;

        if event::poll(Duration::from_millis(100)).map_err(|source| TuiError::Io {
            action: "polling terminal events",
            source,
        })? {
            match event::read().map_err(|source| TuiError::Io {
                action: "reading terminal events",
                source,
            })? {
                Event::Paste(text) if app.view == AppView::Chat => {
                    app.insert_chat_text(&text);
                }
                Event::Key(key) => {
                    if is_ctrl_c(&key) {
                        if app.confirm_exit_request() {
                            break;
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Esc => app.go_back(),
                        KeyCode::Left
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            app.move_chat_cursor_word_left();
                        }
                        KeyCode::Right
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            app.move_chat_cursor_word_right();
                        }
                        KeyCode::Left if app.view == AppView::Chat => app.move_chat_cursor_left(),
                        KeyCode::Right if app.view == AppView::Chat => {
                            app.move_chat_cursor_right();
                        }
                        KeyCode::Home if app.view == AppView::Chat => {
                            app.move_chat_cursor_start();
                        }
                        KeyCode::End if app.view == AppView::Chat => {
                            app.move_chat_cursor_end();
                        }
                        KeyCode::Delete if app.view == AppView::Chat => {
                            app.delete_chat_forward_char();
                        }
                        KeyCode::Left => app.select_left(),
                        KeyCode::Right => app.select_right(),
                        KeyCode::Up => app.select_up(),
                        KeyCode::Down => app.select_down(),
                        KeyCode::PageUp if app.view == AppView::Chat => app.scroll_chat_up(10),
                        KeyCode::PageDown if app.view == AppView::Chat => app.scroll_chat_down(10),
                        KeyCode::Enter if app.view == AppView::Chat && is_multiline_enter(&key) => {
                            app.insert_chat_newline();
                        }
                        KeyCode::Enter => match app.view {
                            AppView::Chat => app.submit_chat_input(&transport, &chat_tx).await,
                            AppView::Status => app.enter_current_view(),
                            AppView::Agent => {
                                app.enter_current_view();
                                app.sync_agent_picker_selection(&transport).await;
                            }
                            AppView::StatusDetail => {}
                        },
                        KeyCode::Char('u' | 'U')
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.clear_chat_input();
                        }
                        KeyCode::Char('w' | 'W')
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.delete_chat_word();
                        }
                        KeyCode::Char('k' | 'K')
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.delete_chat_to_end();
                        }
                        KeyCode::Char('a' | 'A')
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.move_chat_cursor_start();
                        }
                        KeyCode::Char('e' | 'E')
                            if app.view == AppView::Chat
                                && key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.move_chat_cursor_end();
                        }
                        KeyCode::Backspace if app.view == AppView::Chat => {
                            app.delete_chat_char();
                        }
                        KeyCode::Char(ch)
                            if app.view == AppView::Chat
                                && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.insert_chat_text(&ch.to_string());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    chat_task.abort();
    runtime_task.abort();
    Ok(())
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_multiline_enter(key: &KeyEvent) -> bool {
    key.modifiers
        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
}

fn enter_terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, TerminalGuard), TuiError> {
    enable_raw_mode().map_err(|source| TuiError::Io {
        action: "enabling raw mode",
        source,
    })?;
    let mut stdout = io::stdout();
    if let Err(source) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste) {
        let _ = disable_raw_mode();
        return Err(TuiError::Io {
            action: "entering alternate screen",
            source,
        });
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok((terminal, TerminalGuard)),
        Err(source) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
            Err(TuiError::Io {
                action: "creating terminal",
                source,
            })
        }
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

fn print_once(endpoint: &ServerEndpoint, snapshot: &DashboardSnapshot) {
    println!("endpoint: {}", endpoint.base_url());
    if let Some(service) = &snapshot.service {
        println!(
            "service: {} {} mode={} port={}",
            service.service, service.version, service.mode, service.port
        );
    }
    println!(
        "channels: {} tunnels: {} agents: {} sessions: {}",
        snapshot.channels.len(),
        snapshot.tunnels.len(),
        snapshot.agents.len(),
        snapshot.sessions.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_ctrl_c_as_exit_key() {
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let plain_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);

        assert!(is_ctrl_c(&ctrl_c));
        assert!(!is_ctrl_c(&plain_c));
        assert!(!is_ctrl_c(&ctrl_q));
    }

    #[test]
    fn recognizes_modified_enter_as_multiline_input() {
        let plain_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        let alt_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);

        assert!(!is_multiline_enter(&plain_enter));
        assert!(is_multiline_enter(&shift_enter));
        assert!(is_multiline_enter(&alt_enter));
    }
}
