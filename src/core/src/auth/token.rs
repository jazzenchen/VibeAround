//! Local server auth token — per-session, regenerated on every daemon start.
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
//! - Every daemon start generates a fresh 32-byte token from `OsRng`.
//! - The dashboard token is hex-encoded (64 chars) and written to
//!   `~/.vibearound/auth.json` with mode `0600` on Unix.
//! - Separate daemon-lifetime tokens authorize the provider bridge
//!   (`~/.vibearound/local-api-auth.json`) and agent-as-API
//!   (`~/.vibearound/local-agent-api-auth.json`) route families. Giving an
//!   agent a bridge credential does not grant access to dashboard/control
//!   routes or permission to launch another agent.
//! - MCP uses its own daemon-lifetime token in
//!   `~/.vibearound/auth-mcp.json`; coding agents never receive the dashboard
//!   owner token.
//! - `auth.json` stores `{ "port": <u16>, "token": "<hex>" }` so the Tauri
//!   tray and desktop-ui can discover both values without a side channel.
//! - The HTTP layer enforces the token on every protected route via a
//!   middleware that accepts it as `Authorization: Bearer <token>` or as a
//!   `?token=<token>` query parameter (for browser initial-load and for
//!   WebSocket upgrades, which cannot carry custom headers).
//! - Restart invalidates the previous token — sessions in old browser tabs
//!   will 401 and the user reloads the tray's "Open Local Dashboard" entry.

use std::fs;
use std::path::PathBuf;

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

/// File record written to `~/.vibearound/auth.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFile {
    pub port: u16,
    pub token: String,
}

/// Path of the auth token file: `~/.vibearound/auth.json`.
pub fn token_file_path() -> PathBuf {
    config::data_dir().join("auth.json")
}

/// Path of the MCP-only token file.
pub fn mcp_token_file_path() -> PathBuf {
    config::data_dir().join("auth-mcp.json")
}

/// Path of the local API bridge token file.
pub fn local_api_token_file_path() -> PathBuf {
    config::data_dir().join("local-api-auth.json")
}

/// Path of the local agent API token file.
pub fn local_agent_api_token_file_path() -> PathBuf {
    config::data_dir().join("local-agent-api-auth.json")
}

/// Write the auth token file with owner-only permissions on Unix.
///
/// Overwrites any prior file. Callers should invoke this once at daemon
/// start, after the token has been generated.
pub fn write_token_file(port: u16, token: &AuthToken) -> std::io::Result<()> {
    write_auth_file(&token_file_path(), port, token)
}

/// Write the scoped MCP token file.
pub fn write_mcp_token_file(port: u16, token: &AuthToken) -> std::io::Result<()> {
    write_auth_file(&mcp_token_file_path(), port, token)
}

/// Write the scoped local API bridge token file.
pub fn write_local_api_token_file(port: u16, token: &AuthToken) -> std::io::Result<()> {
    write_auth_file(&local_api_token_file_path(), port, token)
}

/// Write the scoped local agent API token file.
pub fn write_local_agent_api_token_file(port: u16, token: &AuthToken) -> std::io::Result<()> {
    write_auth_file(&local_agent_api_token_file_path(), port, token)
}

fn write_auth_file(path: &std::path::Path, port: u16, token: &AuthToken) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let record = AuthFile {
        port,
        token: token.as_str().to_string(),
    };
    let body = serde_json::to_string_pretty(&record).map_err(std::io::Error::other)?;
    fs::write(path, body)?;
    set_owner_only(path)?;
    Ok(())
}

/// Read the auth token file, if it exists and is well-formed.
pub fn read_token_file() -> Option<AuthFile> {
    read_auth_file(&token_file_path())
}

/// Read the scoped MCP token file, if present and well-formed.
pub fn read_mcp_token_file() -> Option<AuthFile> {
    read_auth_file(&mcp_token_file_path())
}

/// Read the scoped local API bridge token file, if present and well-formed.
pub fn read_local_api_token_file() -> Option<AuthFile> {
    read_auth_file(&local_api_token_file_path())
}

/// Read the scoped local agent API token file, if present and well-formed.
pub fn read_local_agent_api_token_file() -> Option<AuthFile> {
    read_auth_file(&local_agent_api_token_file_path())
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
