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
//! 7. Status endpoint returns auth token → frontend stores it in cookie
//!
//! Codes expire after 1 minute. The frontend shows a countdown and a
//! "refresh" button to generate a new code when the old one expires.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use rand::rngs::OsRng;
use rand::Rng;
use uuid::Uuid;

/// How long a pairing code stays valid.
const CODE_TTL: Duration = Duration::from_secs(60);

/// In-memory store of pending pair sessions.
static STORE: LazyLock<Mutex<HashMap<String, PairEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct PairEntry {
    code: String,
    verified: bool,
    expires_at: Instant,
}

/// Generate a new 6-digit pairing code tied to a fresh session ID.
///
/// Returns `(session_id, code)`. The session ID is a UUID that the
/// frontend uses to poll for verification status.
pub fn generate() -> (String, String) {
    let session_id = Uuid::new_v4().to_string();
    let code = random_6_digits();
    let entry = PairEntry {
        code: code.clone(),
        verified: false,
        expires_at: Instant::now() + CODE_TTL,
    };

    let mut store = STORE.lock().unwrap();
    purge_expired(&mut store);
    store.insert(session_id.clone(), entry);

    (session_id, code)
}

/// Validate a pairing code submitted via IM `/pair` command.
///
/// Searches all pending (non-expired, non-verified) sessions for a
/// matching code. On match, marks the session as verified and returns
/// the daemon's auth token. Returns `None` if no match or expired.
pub fn validate(code: &str) -> Option<String> {
    let code = code.trim();
    let mut store = STORE.lock().unwrap();
    purge_expired(&mut store);

    // Find the session with the matching code.
    let session_id = store
        .iter()
        .find(|(_, e)| !e.verified && e.code == code)
        .map(|(sid, _)| sid.clone());

    let session_id = session_id?;
    let entry = store.get_mut(&session_id)?;
    entry.verified = true;

    // Return the auth token from disk.
    super::token::read_token_file().map(|f| f.token)
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

/// Consume a verified session, returning the auth token.
///
/// Once consumed, the session is removed from the store. This prevents
/// the token from being retrieved more than once per pairing.
pub fn consume_verified(session_id: &str) -> Option<String> {
    let mut store = STORE.lock().unwrap();
    purge_expired(&mut store);

    let entry = store.get(session_id)?;
    if !entry.verified {
        return None;
    }
    store.remove(session_id);

    super::token::read_token_file().map(|f| f.token)
}

/// Generate a 6-digit numeric string using the OS CSPRNG.
fn random_6_digits() -> String {
    let n: u32 = OsRng.gen_range(0..1_000_000);
    format!("{:06}", n)
}

/// Remove expired entries from the store.
fn purge_expired(store: &mut HashMap<String, PairEntry>) {
    let now = Instant::now();
    store.retain(|_, e| e.expires_at > now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_returns_6_digit_code() {
        let (sid, code) = generate();
        assert!(!sid.is_empty());
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn validate_matches_and_marks_verified() {
        let (sid, code) = generate();
        // Before validation, status is pending.
        assert_eq!(check_status(&sid), Some(false));

        // Wrong code should fail.
        assert!(validate("000000").is_none());
        assert_eq!(check_status(&sid), Some(false));

        // Note: validate() reads token from disk which may not exist in test.
        // We test the matching logic, not the token retrieval.
        let _ = validate(&code);
        assert_eq!(check_status(&sid), Some(true));
    }

    #[test]
    fn validate_trims_pairing_code() {
        let (sid, code) = generate();
        let _ = validate(&format!("  {code}  "));
        assert_eq!(check_status(&sid), Some(true));
    }

    #[test]
    fn consume_verified_removes_session() {
        let (sid, code) = generate();
        let _ = validate(&code);
        assert_eq!(check_status(&sid), Some(true));

        // Consume removes the session.
        let _ = consume_verified(&sid);
        assert_eq!(check_status(&sid), None);
    }

    #[test]
    fn unknown_session_returns_none() {
        assert_eq!(check_status("nonexistent"), None);
    }
}
