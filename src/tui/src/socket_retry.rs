//! Retry policy shared by the chat and runtime websockets.
//!
//! The daemon rotates its auth token on every restart, so a socket loop must
//! re-read the token and recompute its URL before each attempt. Retries run at
//! a fixed interval; after enough consecutive connect failures the loop stops
//! and parks in a terminal disconnected state until the user asks to
//! reconnect.

use std::time::Duration;

/// Fixed wait between reconnect attempts.
pub(crate) const SOCKET_RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// Consecutive connect failures after which a socket stops retrying.
pub(crate) const SOCKET_RETRY_LIMIT: u32 = 5;

/// What a socket loop should do after a failed connect attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SocketRetry {
    RetryAfter(Duration),
    GiveUp,
}

pub(crate) fn socket_retry_after_failure(consecutive_failures: u32) -> SocketRetry {
    if consecutive_failures >= SOCKET_RETRY_LIMIT {
        SocketRetry::GiveUp
    } else {
        SocketRetry::RetryAfter(SOCKET_RETRY_INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_at_a_fixed_interval_below_the_limit() {
        for failures in 1..SOCKET_RETRY_LIMIT {
            assert_eq!(
                socket_retry_after_failure(failures),
                SocketRetry::RetryAfter(SOCKET_RETRY_INTERVAL),
                "attempt {failures} should retry"
            );
        }
    }

    #[test]
    fn gives_up_at_the_limit() {
        assert_eq!(
            socket_retry_after_failure(SOCKET_RETRY_LIMIT),
            SocketRetry::GiveUp
        );
        assert_eq!(
            socket_retry_after_failure(SOCKET_RETRY_LIMIT + 1),
            SocketRetry::GiveUp
        );
    }
}
