use std::io::{Read, Write};

use crossterm::terminal;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use va_client::events::{encode_pty_client_message, pty_resize, pty_ws_for_session};
use va_client::http::AuthRequirement;

use crate::args::Options;
use crate::config::endpoint_for;
use crate::error::CliError;

const DETACH_BYTE: u8 = 0x1d; // Ctrl-]

enum InputEvent {
    Bytes(Vec<u8>),
    Detach,
}

pub(crate) async fn attach_session(options: &Options, session_id: &str) -> Result<(), CliError> {
    if options.json {
        return Err(CliError::Usage(
            "session attach does not support --json".into(),
        ));
    }

    let endpoint = endpoint_for(options, AuthRequirement::BearerToken)?;
    let socket = pty_ws_for_session(session_id);
    let url = endpoint.websocket_url(&socket);
    let (ws, _) = connect_async(&url)
        .await
        .map_err(|source| ws_error(&url, source))?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    if let Ok((cols, rows)) = terminal::size() {
        let resize = encode_pty_client_message(&pty_resize(cols, rows))?;
        ws_tx
            .send(Message::Text(resize.into()))
            .await
            .map_err(|source| ws_error(&url, source))?;
    }

    eprintln!("attached to {session_id}; press Ctrl-] to detach");
    let _raw_mode = RawModeGuard::enable()?;
    let (input_tx, mut input_rx) = mpsc::channel(32);
    spawn_stdin_reader(input_tx);

    loop {
        tokio::select! {
            maybe = input_rx.recv() => {
                match maybe {
                    Some(InputEvent::Bytes(bytes)) => {
                        ws_tx
                            .send(Message::Binary(bytes.into()))
                            .await
                            .map_err(|source| ws_error(&url, source))?;
                    }
                    Some(InputEvent::Detach) | None => {
                        let _ = ws_tx.close().await;
                        break;
                    }
                }
            }
            maybe = ws_rx.next() => {
                match maybe {
                    Some(Ok(Message::Binary(bytes))) => write_stdout(&bytes)?,
                    Some(Ok(Message::Text(text))) => handle_text_frame(&text)?,
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(source)) => return Err(ws_error(&url, source)),
                }
            }
        }
    }

    Ok(())
}

fn spawn_stdin_reader(tx: mpsc::Sender<InputEvent>) {
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buffer = [0_u8; 4096];
        while let Ok(size) = stdin.read(&mut buffer) {
            if size == 0 {
                break;
            }
            let chunk = &buffer[..size];
            if let Some(detach_at) = chunk.iter().position(|byte| *byte == DETACH_BYTE) {
                if detach_at > 0
                    && tx
                        .blocking_send(InputEvent::Bytes(chunk[..detach_at].to_vec()))
                        .is_err()
                {
                    break;
                }
                let _ = tx.blocking_send(InputEvent::Detach);
                break;
            }
            if tx.blocking_send(InputEvent::Bytes(chunk.to_vec())).is_err() {
                break;
            }
        }
    });
}

fn write_stdout(bytes: &[u8]) -> Result<(), CliError> {
    let mut stdout = std::io::stdout();
    stdout.write_all(bytes).map_err(|source| CliError::Io {
        action: "writing PTY output",
        source,
    })?;
    stdout.flush().map_err(|source| CliError::Io {
        action: "flushing PTY output",
        source,
    })
}

fn handle_text_frame(text: &str) -> Result<(), CliError> {
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        return Ok(());
    }
    let mut stderr = std::io::stderr();
    writeln!(stderr, "\r\n{text}").map_err(|source| CliError::Io {
        action: "writing websocket text frame",
        source,
    })
}

fn ws_error(url: &str, source: tokio_tungstenite::tungstenite::Error) -> CliError {
    CliError::WebSocket {
        url: url.to_string(),
        source,
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Result<Self, CliError> {
        terminal::enable_raw_mode().map_err(|source| CliError::Io {
            action: "enabling raw terminal mode",
            source,
        })?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}
