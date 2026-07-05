//! Short-lived handoff codes for attaching an external agent session to a
//! workspace thread.

use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::Rng;

struct HandoffEntry {
    agent_kind: String,
    profile_id: Option<String>,
    session_id: String,
    cwd: String,
    expires_at: Instant,
}

static HANDOFF_CODES: LazyLock<Mutex<HashMap<String, HandoffEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static HANDOFF_FAILED_ATTEMPTS: LazyLock<Mutex<VecDeque<Instant>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

const TTL: Duration = Duration::from_secs(120);
const FAILED_ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILED_ATTEMPTS: usize = 10;
const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffPayload {
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

pub fn store(payload: HandoffPayload) -> String {
    let mut map = HANDOFF_CODES.lock();
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
        HandoffEntry {
            agent_kind: payload.agent_kind,
            profile_id: payload.profile_id,
            session_id: payload.session_id,
            cwd: payload.cwd,
            expires_at: now + TTL,
        },
    );
    code
}

pub fn consume(code: &str) -> Option<HandoffPayload> {
    let now = Instant::now();
    if failed_attempt_limit_reached(now) {
        tracing::warn!(
            window_secs = FAILED_ATTEMPT_WINDOW.as_secs(),
            max_attempts = MAX_FAILED_ATTEMPTS,
            "handoff pickup rejected after too many failed attempts"
        );
        return None;
    }

    let mut map = HANDOFF_CODES.lock();
    map.retain(|_, entry| entry.expires_at > now);
    let Some(entry) = map.remove(&code.to_uppercase()) else {
        drop(map);
        record_failed_attempt(now);
        return None;
    };
    Some(HandoffPayload {
        agent_kind: entry.agent_kind,
        profile_id: entry.profile_id,
        session_id: entry.session_id,
        cwd: entry.cwd,
    })
}

fn failed_attempt_limit_reached(now: Instant) -> bool {
    let mut attempts = HANDOFF_FAILED_ATTEMPTS.lock();
    prune_failed_attempts(&mut attempts, now);
    attempts.len() >= MAX_FAILED_ATTEMPTS
}

fn record_failed_attempt(now: Instant) {
    let mut attempts = HANDOFF_FAILED_ATTEMPTS.lock();
    prune_failed_attempts(&mut attempts, now);
    attempts.push_back(now);
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

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_for_test() {
        HANDOFF_CODES.lock().clear();
        HANDOFF_FAILED_ATTEMPTS.lock().clear();
    }

    #[test]
    fn generated_code_is_four_chars_from_alphabet() {
        let _guard = TEST_LOCK.lock();
        reset_for_test();
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
        let _guard = TEST_LOCK.lock();
        reset_for_test();
        let code = store(HandoffPayload {
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
        let _guard = TEST_LOCK.lock();
        reset_for_test();
        let code = store(HandoffPayload {
            agent_kind: "gemini".into(),
            profile_id: None,
            session_id: "sess-2".into(),
            cwd: "/home".into(),
        });
        assert!(consume(&code).is_some());
        assert!(consume(&code).is_none(), "second consume must fail");
    }

    #[test]
    fn failed_pickups_are_rate_limited() {
        let _guard = TEST_LOCK.lock();
        reset_for_test();

        for attempt in 0..MAX_FAILED_ATTEMPTS {
            assert!(consume(&format!("MISS{attempt}")).is_none());
        }
        let valid_code = store(HandoffPayload {
            agent_kind: "claude".into(),
            profile_id: Some("default".into()),
            session_id: "sess-1".into(),
            cwd: "/tmp".into(),
        });

        assert!(consume(&valid_code).is_none());
    }
}
