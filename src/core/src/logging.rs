//! Global tracing subscriber setup.
//!
//! [`init`] installs the subscriber with `try_init`; repeated calls return an
//! error without panicking.
//!
//! # Runtime controls
//!
//! The filter is driven by `RUST_LOG`. Without it, the default is
//! `info,common=debug` — the kernel gets verbose logs, everything else
//! (tokio internals, hyper, ACP crate) stays at info. Override via the
//! environment, e.g.
//!
//! ```sh
//! RUST_LOG=warn,common::channels=trace cargo run
//! ```

use std::sync::OnceLock;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Install the global tracing subscriber. Safe to call multiple times —
/// only the first call takes effect.
pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,common=debug"));

    let file_layer = {
        let log_dir = crate::config::data_dir().join("logs").join("runtime");
        std::fs::create_dir_all(&log_dir).ok().map(|_| {
            let file_appender = tracing_appender::rolling::daily(log_dir, "vibearound.log");
            let (writer, guard) = tracing_appender::non_blocking(file_appender);
            let _ = LOG_GUARD.set(guard);

            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_line_number(false)
                .with_ansi(false)
                .with_writer(writer)
        })
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_line_number(false),
        )
        .with(file_layer)
        .try_init();
}
