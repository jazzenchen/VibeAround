//! Short-lived handover codes for attaching an external agent session to a
//! workspace thread.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::Rng;

struct HandoverEntry {
    agent_kind: String,
    profile_id: Option<String>,
    session_id: String,
    cwd: String,
    expires_at: Instant,
}

static HANDOVER_CODES: LazyLock<Mutex<HashMap<String, HandoverEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const TTL: Duration = Duration::from_secs(120);
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

pub fn store(payload: HandoverPayload) -> String {
    let mut map = HANDOVER_CODES.lock();
    let now = Instant::now();
    map.retain(|_, entry| entry.expires_at > now);

    let code = loop {
        let candidate = generate_code();
        if !map.contains_key(&candidate) {
            break candidate;
        }
    };
    map.insert(
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

pub fn consume(code: &str) -> Option<HandoverPayload> {
    let mut map = HANDOVER_CODES.lock();
    let now = Instant::now();
    map.retain(|_, entry| entry.expires_at > now);
    let entry = map.remove(&code.to_uppercase())?;
    Some(HandoverPayload {
        agent_kind: entry.agent_kind,
        profile_id: entry.profile_id,
        session_id: entry.session_id,
        cwd: entry.cwd,
    })
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
        let code = store(HandoverPayload {
            agent_kind: "claude".into(),
            profile_id: Some("deepseek".into()),
            session_id: "sess-1".into(),
            cwd: "/tmp".into(),
        });
        let payload = consume(&code).expect("code should resolve");
        assert_eq!(payload.agent_kind, "claude");
        assert_eq!(payload.profile_id.as_deref(), Some("deepseek"));
        assert_eq!(payload.session_id, "sess-1");
        assert_eq!(payload.cwd, "/tmp");
    }

    #[test]
    fn consume_is_one_shot() {
        let code = store(HandoverPayload {
            agent_kind: "gemini".into(),
            profile_id: None,
            session_id: "sess-2".into(),
            cwd: "/home".into(),
        });
        assert!(consume(&code).is_some());
        assert!(consume(&code).is_none(), "second consume must fail");
    }
}
