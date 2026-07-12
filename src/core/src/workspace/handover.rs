//! Short-lived handover codes for attaching an external agent session to a
//! workspace thread.

use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use rand::rngs::OsRng;
use rand::Rng;
use tokio::sync::{mpsc, oneshot};

struct HandoverEntry {
    agent_kind: String,
    profile_id: Option<String>,
    session_id: String,
    cwd: String,
    expires_at: Instant,
}

#[derive(Default)]
struct HandoverState {
    codes: HashMap<String, HandoverEntry>,
    failed_attempts: VecDeque<Instant>,
}

enum HandoverCommand {
    Store {
        payload: HandoverPayload,
        reply: oneshot::Sender<String>,
    },
    Consume {
        code: String,
        reply: oneshot::Sender<Option<HandoverPayload>>,
    },
}

const TTL: Duration = Duration::from_secs(120);
const FAILED_ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILED_ATTEMPTS: usize = 10;
const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverPayload {
    pub agent_kind: String,
    pub profile_id: Option<String>,
    pub session_id: String,
    pub cwd: String,
}

fn generate_code() -> String {
    let mut rng = OsRng;
    (0..4)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

pub async fn store(payload: HandoverPayload) -> String {
    let (reply, code) = oneshot::channel();
    command_tx()
        .send(HandoverCommand::Store { payload, reply })
        .expect("handover owner must outlive its command sender");
    code.await.expect("handover owner must reply")
}

pub async fn consume(code: &str) -> Option<HandoverPayload> {
    let (reply, payload) = oneshot::channel();
    command_tx()
        .send(HandoverCommand::Consume {
            code: code.to_string(),
            reply,
        })
        .expect("handover owner must outlive its command sender");
    payload.await.expect("handover owner must reply")
}

fn command_tx() -> &'static mpsc::UnboundedSender<HandoverCommand> {
    static COMMAND_TX: OnceLock<mpsc::UnboundedSender<HandoverCommand>> = OnceLock::new();
    COMMAND_TX.get_or_init(|| {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        tokio::spawn(run_owner(command_rx));
        command_tx
    })
}

async fn run_owner(mut command_rx: mpsc::UnboundedReceiver<HandoverCommand>) {
    let mut state = HandoverState::default();
    while let Some(command) = command_rx.recv().await {
        match command {
            HandoverCommand::Store { payload, reply } => {
                let _ = reply.send(state.store(payload));
            }
            HandoverCommand::Consume { code, reply } => {
                let _ = reply.send(state.consume(&code));
            }
        }
    }
}

impl HandoverState {
    fn store(&mut self, payload: HandoverPayload) -> String {
        let now = Instant::now();
        self.codes.retain(|_, entry| entry.expires_at > now);
        let code = loop {
            let candidate = generate_code();
            if !self.codes.contains_key(&candidate) {
                break candidate;
            }
        };
        self.codes.insert(
            code.clone(),
            HandoverEntry {
                agent_kind: payload.agent_kind,
                profile_id: payload.profile_id,
                session_id: payload.session_id,
                cwd: payload.cwd,
                expires_at: now + TTL,
            },
        );
        code
    }

    fn consume(&mut self, code: &str) -> Option<HandoverPayload> {
        let now = Instant::now();
        prune_failed_attempts(&mut self.failed_attempts, now);
        if self.failed_attempts.len() >= MAX_FAILED_ATTEMPTS {
            tracing::warn!(
                window_secs = FAILED_ATTEMPT_WINDOW.as_secs(),
                max_attempts = MAX_FAILED_ATTEMPTS,
                "handover pickup rejected after too many failed attempts"
            );
            return None;
        }
        self.codes.retain(|_, entry| entry.expires_at > now);
        let Some(entry) = self.codes.remove(&code.to_uppercase()) else {
            self.failed_attempts.push_back(now);
            return None;
        };
        Some(HandoverPayload {
            agent_kind: entry.agent_kind,
            profile_id: entry.profile_id,
            session_id: entry.session_id,
            cwd: entry.cwd,
        })
    }
}

fn prune_failed_attempts(attempts: &mut VecDeque<Instant>, now: Instant) {
    while attempts
        .front()
        .is_some_and(|attempt| now.duration_since(*attempt) >= FAILED_ATTEMPT_WINDOW)
    {
        attempts.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_code_is_four_chars_from_alphabet() {
        for _ in 0..100 {
            let code = generate_code();
            assert_eq!(code.len(), 4);
            for byte in code.bytes() {
                assert!(CHARSET.contains(&byte), "char {byte:?} not in alphabet");
            }
        }
    }

    #[test]
    fn store_and_consume_roundtrip() {
        let mut state = HandoverState::default();
        let code = state.store(HandoverPayload {
            agent_kind: "claude".into(),
            profile_id: Some("deepseek".into()),
            session_id: "sess-1".into(),
            cwd: "/tmp".into(),
        });
        let payload = state.consume(&code).expect("code should resolve");
        assert_eq!(payload.agent_kind, "claude");
        assert_eq!(payload.profile_id.as_deref(), Some("deepseek"));
        assert_eq!(payload.session_id, "sess-1");
        assert_eq!(payload.cwd, "/tmp");
    }

    #[test]
    fn consume_is_one_shot() {
        let mut state = HandoverState::default();
        let code = state.store(HandoverPayload {
            agent_kind: "gemini".into(),
            profile_id: None,
            session_id: "sess-2".into(),
            cwd: "/home".into(),
        });
        assert!(state.consume(&code).is_some());
        assert!(state.consume(&code).is_none(), "second consume must fail");
    }

    #[test]
    fn failed_pickups_are_rate_limited() {
        let mut state = HandoverState::default();

        for attempt in 0..MAX_FAILED_ATTEMPTS {
            assert!(state.consume(&format!("MISS{attempt}")).is_none());
        }
        let valid_code = state.store(HandoverPayload {
            agent_kind: "claude".into(),
            profile_id: Some("default".into()),
            session_id: "sess-1".into(),
            cwd: "/tmp".into(),
        });

        assert!(state.consume(&valid_code).is_none());
    }
}
