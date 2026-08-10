//! Browser pairing — 6-digit code confirmed via IM `/pair` command.
//!
//! ## Flow
//!
//! 1. Browser opens `/va/` → frontend calls `POST /va/api/pair/start`
//! 2. Backend generates a `session_id` (UUID) + 6-digit code, returns both
//! 3. Frontend displays: "Your pairing code: **847291**"
//! 4. User sends `/pair 847291` in any IM channel connected to VibeAround
//! 5. IM handler calls [`validate`] — on match, marks session as verified
//! 6. Frontend polls `GET /va/api/pair/status?sid=...` → detects verified
//! 7. Status endpoint returns the auth token → frontend stores it for API calls;
//!    public hosts also receive a `/va/`-scoped owner cookie
//!
//! Codes expire after 1 minute. The frontend shows a countdown and a
//! "refresh" button to generate a new code when the old one expires.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use rand::rngs::OsRng;
use rand::Rng;
use uuid::Uuid;

/// How long a pairing code stays valid.
pub const CODE_TTL_SECS: u64 = 60;
const CODE_TTL: Duration = Duration::from_secs(CODE_TTL_SECS);
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
const MAX_VALIDATE_ATTEMPTS_PER_WINDOW: usize = 30;
const MAX_PENDING_SESSIONS: usize = 64;
const CODE_SPACE: u32 = 1_000_000;

/// In-memory store of pending pair sessions.
static STORE: LazyLock<Mutex<HashMap<String, PairEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static VALIDATE_ATTEMPTS: LazyLock<Mutex<VecDeque<Instant>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

struct PairEntry {
    code: String,
    verified: bool,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateError {
    TooManyPending,
}

/// Generate a new 6-digit pairing code tied to a fresh session ID.
///
/// Returns `(session_id, code)`. The session ID is a UUID that the
/// frontend uses to poll for verification status.
pub fn generate() -> Result<(String, String), GenerateError> {
    let session_id = Uuid::new_v4().to_string();
    let mut store = STORE.lock().unwrap();
    purge_expired(&mut store);
    if store.len() >= MAX_PENDING_SESSIONS {
        return Err(GenerateError::TooManyPending);
    }
    let code = unique_6_digit_code(&store);
    let entry = PairEntry {
        code: code.clone(),
        verified: false,
        expires_at: Instant::now() + CODE_TTL,
    };

    store.insert(session_id.clone(), entry);

    Ok((session_id, code))
}

/// Validate a pairing code submitted via IM `/pair` command.
///
/// Searches all pending (non-expired, non-verified) sessions for a
/// matching code. On match, marks the session as verified. Returns `false`
/// if no match, expired, or validation is temporarily rate-limited.
pub fn validate(code: &str) -> bool {
    let code = code.trim();
    if !record_validate_attempt() {
        return false;
    }

    let mut store = STORE.lock().unwrap();
    purge_expired(&mut store);

    // Find the session with the matching code.
    let session_id = store
        .iter()
        .find(|(_, e)| !e.verified && e.code == code)
        .map(|(sid, _)| sid.clone());

    let Some(session_id) = session_id else {
        return false;
    };
    let Some(entry) = store.get_mut(&session_id) else {
        return false;
    };
    entry.verified = true;
    clear_validate_attempts();
    true
}

/// Check whether a pairing session has been verified.
///
/// Returns:
/// - `Some(true)` if verified (code was accepted via IM)
/// - `Some(false)` if still pending
/// - `None` if session_id is unknown or expired
pub fn check_status(session_id: &str) -> Option<bool> {
    let mut store = STORE.lock().unwrap();
    purge_expired(&mut store);

    store.get(session_id).map(|e| e.verified)
}

/// Generate an active-code-unique six-digit string. Starting from a random
/// value and scanning the tiny occupied set avoids retry state while keeping
/// codes unpredictable for a single-user pairing flow.
fn unique_6_digit_code(store: &HashMap<String, PairEntry>) -> String {
    let start: u32 = OsRng.gen_range(0..CODE_SPACE);
    for offset in 0..CODE_SPACE {
        let code = format!("{:06}", (start + offset) % CODE_SPACE);
        if store.values().all(|entry| entry.code != code) {
            return code;
        }
    }
    unreachable!("pending pairing sessions are capped below the code space")
}

/// Remove expired entries from the store.
fn purge_expired(store: &mut HashMap<String, PairEntry>) {
    let now = Instant::now();
    store.retain(|_, e| e.expires_at > now);
}

fn record_validate_attempt() -> bool {
    let now = Instant::now();
    let mut attempts = VALIDATE_ATTEMPTS.lock().unwrap();
    while attempts
        .front()
        .is_some_and(|attempt| now.duration_since(*attempt) >= ATTEMPT_WINDOW)
    {
        attempts.pop_front();
    }
    if attempts.len() >= MAX_VALIDATE_ATTEMPTS_PER_WINDOW {
        return false;
    }
    attempts.push_back(now);
    true
}

fn clear_validate_attempts() {
    VALIDATE_ATTEMPTS.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn reset_test_state() {
        STORE.lock().unwrap().clear();
        VALIDATE_ATTEMPTS.lock().unwrap().clear();
    }

    #[test]
    fn generate_returns_6_digit_code() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let (sid, code) = generate().unwrap();
        assert!(!sid.is_empty());
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn validate_matches_and_marks_verified() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let (sid, code) = generate().unwrap();
        // Before validation, status is pending.
        assert_eq!(check_status(&sid), Some(false));

        // Wrong code should fail.
        assert!(!validate("not-the-code"));
        assert_eq!(check_status(&sid), Some(false));

        assert!(validate(&code));
        assert_eq!(check_status(&sid), Some(true));
    }

    #[test]
    fn validate_trims_pairing_code() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let (sid, code) = generate().unwrap();
        assert!(validate(&format!("  {code}  ")));
        assert_eq!(check_status(&sid), Some(true));
    }

    #[test]
    fn verified_session_remains_available_until_expiry() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let (sid, code) = generate().unwrap();
        assert!(validate(&code));
        assert_eq!(check_status(&sid), Some(true));
        assert_eq!(check_status(&sid), Some(true));
    }

    #[test]
    fn unknown_session_returns_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        assert_eq!(check_status("nonexistent"), None);
    }

    #[test]
    fn active_pairing_codes_are_unique() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        let mut pairs = Vec::new();
        for _ in 0..MAX_PENDING_SESSIONS {
            pairs.push(generate().unwrap());
        }
        let mut codes = pairs.into_iter().map(|(_, code)| code).collect::<Vec<_>>();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), MAX_PENDING_SESSIONS);
    }

    #[test]
    fn pending_pairing_sessions_are_bounded() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_test_state();
        for _ in 0..MAX_PENDING_SESSIONS {
            generate().unwrap();
        }
        assert_eq!(generate(), Err(GenerateError::TooManyPending));
    }
}
