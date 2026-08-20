//! Local server auth tokens — per-session for control routes, persistent for
//! the agent-as-API credential users paste into a profile by hand.
//!
//! ## Threat model
//!
//! VibeAround is a single-user desktop app. The web dashboard, MCP endpoint,
//! and WebSocket routes are reachable on `http://127.0.0.1:{port}` and —
//! when the tunnel is enabled — via a public URL. Without auth, any browser
//! tab the user visits can fetch from the loopback port (via DNS rebinding
//! or plain cross-origin requests with `CorsLayer::Any`), and anyone who
//! learns the tunnel URL can spawn a PTY as the user.
//!
//! ## Design
//!
//! - Every daemon start generates a fresh 32-byte token from `OsRng`, except
//!   for the agent-as-API credential (see below).
//! - All four tokens are hex-encoded (64 chars) and live together in
//!   `~/.vibearound/auth.json`, written once with mode `0600` on Unix.
//! - They stay four distinct values: the provider bridge and agent-as-API
//!   route families each accept only their own, so giving an agent a bridge
//!   credential grants no access to dashboard/control routes and no
//!   permission to launch another agent. MCP likewise has its own, and
//!   coding agents never receive the dashboard owner token.
//! - The file stores `{ "port", "token", "mcp_token", "bridge_token",
//!   "agent_token" }`. `port` and `token` keep the names and meaning they had
//!   when each family had its own file, so a client built before the merge
//!   reads the dashboard credential unchanged.
//! - Before 0.8 the scoped tokens lived in `auth-mcp.json`,
//!   `local-api-auth.json` and `local-agent-api-auth.json`. Those are read as
//!   a fallback and deleted on the first merged write; drop that path after
//!   0.8.x.
//! - The HTTP layer enforces the token on every protected route via a
//!   middleware that accepts it as `Authorization: Bearer <token>` or as a
//!   `?token=<token>` query parameter (for browser initial-load and for
//!   WebSocket upgrades, which cannot carry custom headers).
//! - Restart invalidates the previous token — sessions in old browser tabs
//!   will 401 and the user reloads the tray's "Open Local Dashboard" entry.
//! - The agent-as-API credential is the exception: users copy it into a
//!   provider profile by hand, so rotating it on every start would break
//!   every profile that points at a local agent. It is restored from
//!   `local-agent-api-auth.json` on start and only changes when the user asks
//!   for a new one, which invalidates the old one immediately.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::config;

/// An opaque authentication token for the local web server.
///
/// Stored as a hex string. Constructed via [`AuthToken::generate`] and
/// compared with constant-time equality in the middleware layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthToken(String);

impl AuthToken {
    /// Generate a fresh 32-byte token from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(hex_encode(&bytes))
    }

    /// Restore a token persisted by an earlier run.
    ///
    /// Returns `None` for anything that is not a 64-char hex string, so a
    /// truncated or hand-edited file mints a fresh token instead of
    /// installing a weak credential.
    pub fn from_hex(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self(value.to_ascii_lowercase()))
    }

    /// Borrow the token as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Constant-time comparison against a candidate.
    ///
    /// Prevents timing side channels on token comparison. Not critical at
    /// 256 bits of entropy over a loopback socket, but cheap and correct.
    pub fn matches(&self, candidate: &str) -> bool {
        constant_time_eq(self.0.as_bytes(), candidate.as_bytes())
    }
}

/// An [`AuthToken`] shared by request handlers that the user can rotate
/// while the daemon runs.
///
/// Handlers hold a clone, so a rotation takes effect on the next request
/// without a restart.
#[derive(Debug, Clone)]
pub struct SharedAuthToken(Arc<RwLock<AuthToken>>);

impl SharedAuthToken {
    pub fn new(token: AuthToken) -> Self {
        Self(Arc::new(RwLock::new(token)))
    }

    /// Constant-time comparison against a candidate.
    pub fn matches(&self, candidate: &str) -> bool {
        self.read().matches(candidate)
    }

    /// Copy the current token out, for persisting or displaying it.
    pub fn snapshot(&self) -> AuthToken {
        self.read().clone()
    }

    /// Swap in a replacement, invalidating the previous token.
    ///
    /// Callers that also persist the token must write it first: a swap the
    /// disk never received would leave the daemon demanding a credential
    /// nobody holds.
    pub fn replace(&self, token: AuthToken) {
        *self.write() = token;
    }

    fn read(&self) -> RwLockReadGuard<'_, AuthToken> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, AuthToken> {
        self.0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The credential file: `~/.vibearound/auth.json`.
///
/// `port` and `token` keep the names every existing reader already parses —
/// `token` is the dashboard credential — and the scoped families sit beside
/// them, so a client built before the merge still works against a merged file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFile {
    pub port: u16,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_token: Option<String>,
}

/// The four credentials a running daemon holds.
pub struct DaemonTokens<'a> {
    pub dashboard: &'a AuthToken,
    pub mcp: &'a AuthToken,
    pub bridge: &'a AuthToken,
    pub agent: &'a AuthToken,
}

/// Path of the auth file: `~/.vibearound/auth.json`.
pub fn token_file_path() -> PathBuf {
    config::data_dir().join("auth.json")
}

/// Pre-merge path of the MCP token. Read-only; dropped after 0.8.x.
fn legacy_mcp_token_file_path() -> PathBuf {
    config::data_dir().join("auth-mcp.json")
}

/// Pre-merge path of the local API bridge token. Read-only; dropped after 0.8.x.
fn legacy_local_api_token_file_path() -> PathBuf {
    config::data_dir().join("local-api-auth.json")
}

/// Pre-merge path of the agent-as-API token. Read-only; dropped after 0.8.x.
fn legacy_local_agent_api_token_file_path() -> PathBuf {
    config::data_dir().join("local-agent-api-auth.json")
}

/// Write all four credentials, then retire the pre-merge files.
///
/// One write keeps the set consistent: a crash can no longer leave a mix of
/// generations on disk the way four separate writes could.
pub fn write_auth_file(port: u16, tokens: DaemonTokens<'_>) -> std::io::Result<()> {
    let path = token_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let record = AuthFile {
        port,
        token: tokens.dashboard.as_str().to_string(),
        mcp_token: Some(tokens.mcp.as_str().to_string()),
        bridge_token: Some(tokens.bridge.as_str().to_string()),
        agent_token: Some(tokens.agent.as_str().to_string()),
    };
    let body = serde_json::to_string_pretty(&record).map_err(std::io::Error::other)?;
    fs::write(&path, body)?;
    set_owner_only(&path)?;

    // Their contents are now in auth.json; leaving them would keep handing a
    // stale credential to anything that still reads them.
    for legacy in [
        legacy_mcp_token_file_path(),
        legacy_local_api_token_file_path(),
        legacy_local_agent_api_token_file_path(),
    ] {
        let _ = fs::remove_file(legacy);
    }
    Ok(())
}

/// Read the auth file, if it exists and is well-formed.
pub fn read_token_file() -> Option<AuthFile> {
    read_auth_file(&token_file_path())
}

/// The MCP credential, as an [`AuthFile`] so callers read `.port` and `.token`
/// exactly as they did before the merge.
pub fn read_mcp_token_file() -> Option<AuthFile> {
    scoped_token(|file| file.mcp_token.clone(), legacy_mcp_token_file_path)
}

/// The local API bridge credential.
pub fn read_local_api_token_file() -> Option<AuthFile> {
    scoped_token(
        |file| file.bridge_token.clone(),
        legacy_local_api_token_file_path,
    )
}

/// The agent-as-API credential.
pub fn read_local_agent_api_token_file() -> Option<AuthFile> {
    scoped_token(
        |file| file.agent_token.clone(),
        legacy_local_agent_api_token_file_path,
    )
}

/// Take a scoped credential from the merged file, falling back to the file it
/// used to live in so an upgrade does not invalidate it.
fn scoped_token(
    from_merged: impl Fn(&AuthFile) -> Option<String>,
    legacy_path: impl Fn() -> PathBuf,
) -> Option<AuthFile> {
    let merged = read_auth_file(&token_file_path());
    if let Some(file) = &merged {
        if let Some(token) = from_merged(file) {
            return Some(AuthFile {
                port: file.port,
                token,
                mcp_token: None,
                bridge_token: None,
                agent_token: None,
            });
        }
    }
    read_auth_file(&legacy_path())
}

/// Restore the persisted agent-as-API credential, minting one on first run.
///
/// Unlike the other token families this one survives restarts: users paste it
/// into a provider profile by hand, and re-rolling it every start would break
/// those profiles.
pub fn load_or_create_local_agent_api_token() -> AuthToken {
    restore_or_generate(read_local_agent_api_token_file())
}

fn restore_or_generate(persisted: Option<AuthFile>) -> AuthToken {
    persisted
        .and_then(|file| AuthToken::from_hex(&file.token))
        .unwrap_or_else(AuthToken::generate)
}

fn read_auth_file(path: &std::path::Path) -> Option<AuthFile> {
    let body = fs::read_to_string(path).ok()?;
    serde_json::from_str(&body).ok()
}

/// Set a file to mode `0600` on Unix; no-op on Windows (NTFS ACLs are
/// already user-scoped under `%APPDATA%`).
pub fn set_owner_only(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Small helpers (avoid pulling in a dep just for hex + ct compare)
// ---------------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_hex_round_trips_a_generated_token() {
        let token = AuthToken::generate();
        assert_eq!(
            AuthToken::from_hex(token.as_str()).expect("restores a generated token"),
            token
        );
    }

    #[test]
    fn from_hex_rejects_values_that_are_not_tokens() {
        assert!(AuthToken::from_hex("").is_none());
        assert!(AuthToken::from_hex("not-a-token").is_none());
        // Right shape, wrong length — a truncated file must not weaken auth.
        assert!(AuthToken::from_hex(&"a".repeat(63)).is_none());
        assert!(AuthToken::from_hex(&"a".repeat(65)).is_none());
        // Right length, non-hex character.
        assert!(AuthToken::from_hex(&format!("{}z", "a".repeat(63))).is_none());
    }

    #[test]
    fn the_auth_file_keeps_the_field_names_older_clients_parse() {
        let file = AuthFile {
            port: 12358,
            token: "dashboard".to_string(),
            mcp_token: Some("mcp".to_string()),
            bridge_token: Some("bridge".to_string()),
            agent_token: Some("agent".to_string()),
        };
        let body = serde_json::to_string(&file).expect("serializes");

        // A client built before the merge reads exactly these two fields.
        let legacy: serde_json::Value = serde_json::from_str(&body).expect("parses");
        assert_eq!(legacy["port"], 12358);
        assert_eq!(legacy["token"], "dashboard");
    }

    #[test]
    fn a_pre_merge_file_still_parses_as_the_merged_shape() {
        // Every scoped file used to hold exactly `{port, token}`.
        let file: AuthFile =
            serde_json::from_str(r#"{"port":12358,"token":"only"}"#).expect("parses");

        assert_eq!(file.token, "only");
        assert!(file.mcp_token.is_none());
        assert!(file.agent_token.is_none());
    }

    #[test]
    fn a_persisted_token_is_restored_rather_than_reminted() {
        let persisted = AuthToken::generate();
        let restored = restore_or_generate(Some(AuthFile {
            port: 12358,
            token: persisted.as_str().to_string(),
            mcp_token: None,
            bridge_token: None,
            agent_token: None,
        }));
        assert_eq!(restored, persisted);
    }

    #[test]
    fn a_missing_or_corrupt_token_file_mints_a_fresh_one() {
        assert_eq!(restore_or_generate(None).as_str().len(), 64);
        let corrupt = restore_or_generate(Some(AuthFile {
            port: 12358,
            token: "truncated".to_string(),
            mcp_token: None,
            bridge_token: None,
            agent_token: None,
        }));
        assert_eq!(corrupt.as_str().len(), 64);
    }

    #[test]
    fn replacing_a_shared_token_invalidates_the_previous_value() {
        let shared = SharedAuthToken::new(AuthToken::generate());
        let previous = shared.snapshot();
        assert!(shared.matches(previous.as_str()));

        let next = AuthToken::generate();
        shared.replace(next.clone());
        assert!(!shared.matches(previous.as_str()));
        assert!(shared.matches(next.as_str()));
        assert_eq!(shared.snapshot(), next);
    }

    #[test]
    fn shared_token_clones_observe_a_replacement() {
        let shared = SharedAuthToken::new(AuthToken::generate());
        let handler_copy = shared.clone();
        let next = AuthToken::generate();
        shared.replace(next.clone());
        assert!(handler_copy.matches(next.as_str()));
    }

    #[test]
    fn generate_produces_64_hex_chars() {
        let t = AuthToken::generate();
        assert_eq!(t.as_str().len(), 64);
        assert!(t.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_is_unique_across_calls() {
        let a = AuthToken::generate();
        let b = AuthToken::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn matches_rejects_wrong_token() {
        let t = AuthToken::generate();
        assert!(t.matches(t.as_str()));
        assert!(!t.matches("0000000000000000000000000000000000000000000000000000000000000000"));
        assert!(!t.matches(""));
        assert!(!t.matches("short"));
    }

    #[test]
    fn hex_encode_roundtrip() {
        assert_eq!(hex_encode(&[0x00]), "00");
        assert_eq!(hex_encode(&[0xff]), "ff");
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }
}
