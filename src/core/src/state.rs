//! Uniform state-inspection surface every kernel manager implements.
//!
//! This is the contract between `common` and its shells (the axum
//! server, the Tauri desktop, any future TUI/CLI). Every manager that
//! holds runtime state — `ChannelMonitor`, `WorkspaceThreadManager`,
//! `TunnelManager` — implements [`StateSource`] so consumers have two
//! ways to work with it:
//!
//! - **Poll**: call [`StateSource::list`] whenever you need the current
//!   set of entries. Cheap; safe to call at polling cadence.
//! - **Subscribe**: call [`StateSource::subscribe_changes`] to get a
//!   `broadcast::Receiver<()>` that pings when `list()` output
//!   changes, then re-poll. No typed event payloads on this channel
//!   by design — every additional schema is an additional thing that
//!   can drift from the list entries. Managers that need richer typed
//!   events can expose them on separate channels; `subscribe_changes` is
//!   the lowest-common-denominator signal.
//!
//! # Why not take a trait bound at every call site?
//!
//! The trait is deliberately simple — no lifetime parameters, no
//! associated `Event`. That means trait objects are rarely useful
//! (every manager's `Entry` differs). In practice consumers hold
//! concrete references (`Arc<ChannelMonitor>`, etc.) and the trait
//! documents what they can count on.
//!

/// Managers that expose a list of entries and notify when the list
/// changes. See module docs for the intended usage pattern.
///
/// `#[allow(async_fn_in_trait)]`: the trait is only used via concrete
/// manager handles (`Arc<ChannelMonitor>` etc.) — never as a trait object
/// — so the missing `Send` bound in the desugared return type is
/// inferred at each call site and causes no practical issue. The trade
/// of readability (`async fn list`) for a one-off advisory is worth it
/// for an internal contract.
#[allow(async_fn_in_trait)]
pub trait StateSource {
    /// Entry type — typically `Arc<SomeRuntimeObject>` for long-lived
    /// entities whose fields are read live (pods, tunnels) or a computed
    /// value struct for derived views (channel status with relative
    /// timestamps).
    type Entry;

    /// Current state. `async` because most managers hold their entries
    /// behind `tokio::sync::Mutex` / `RwLock` / `ArcSwap`; implementers
    /// that don't need to await (e.g. the channel monitor which reads
    /// from sync atomics) just return immediately — the runtime cost is
    /// nil.
    async fn list(&self) -> Vec<Self::Entry>;

    /// Subscribe to change notifications. Each `()` means "call
    /// `list()` again to see the new state". No payload: the trait
    /// refuses to accrete a second schema.
    ///
    /// Subscription itself is a cheap atomic op, so this stays sync.
    /// Lagged receivers should not treat lag as an error — the next
    /// `list()` is always authoritative. See the `tokio::sync::broadcast`
    /// docs for `RecvError::Lagged` handling.
    fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<()>;
}
