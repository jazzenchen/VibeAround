use std::io;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
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
mod selection;
mod theme;
mod transport;

use app::{AppView, TuiApp};
use chat_socket::{run_chat_socket, ChatSocketEvent};
use config::{resolve_endpoint, Args, RuntimeEnv};
use data::{fetch_snapshot, DashboardSnapshot};
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

    loop {
        while let Ok(event) = socket_event_rx.try_recv() {
            app.apply_chat_socket_event(event);
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
            if let Event::Key(key) = event::read().map_err(|source| TuiError::Io {
                action: "reading terminal events",
                source,
            })? {
                if is_ctrl_c(&key) {
                    if app.confirm_exit_request() {
                        break;
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Esc => app.go_back(),
                    KeyCode::Left => app.select_left(),
                    KeyCode::Right => app.select_right(),
                    KeyCode::Up => app.select_up(),
                    KeyCode::Down => app.select_down(),
                    KeyCode::PageUp if app.view == AppView::Chat => app.scroll_chat_up(10),
                    KeyCode::PageDown if app.view == AppView::Chat => app.scroll_chat_down(10),
                    KeyCode::Enter => match app.view {
                        AppView::Chat => app.submit_chat_input(&transport, &chat_tx).await,
                        AppView::Status | AppView::Agent => app.enter_current_view(),
                        AppView::StatusDetail => {}
                    },
                    KeyCode::Backspace if app.view == AppView::Chat => {
                        app.chat_input.pop();
                    }
                    KeyCode::Char(ch)
                        if app.view == AppView::Chat
                            && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        app.chat_input.push(ch);
                    }
                    _ => {}
                }
            }
        }
    }

    chat_task.abort();
    Ok(())
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn enter_terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, TerminalGuard), TuiError> {
    enable_raw_mode().map_err(|source| TuiError::Io {
        action: "enabling raw mode",
        source,
    })?;
    let mut stdout = io::stdout();
    if let Err(source) = execute!(stdout, EnterAlternateScreen) {
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
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
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
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
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
}
