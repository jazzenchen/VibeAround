//! Conversation registry for the stateful agent-as-API mode.
//!
//! A conversation maps a client-supplied key to one persistent ACP session on
//! one (live or respawnable) agent. Keys come from two places: the Responses
//! protocol chains `previous_response_id`s the bridge mints, and the chat /
//! messages protocols carry an explicit `x-vibearound-conversation` header.
//! The registry is in-memory by design: a daemon restart forgets it, after
//! which header-keyed clients reseed transparently from the full history they
//! send on every turn anyway, and Responses chains answer 404 so the client
//! falls back to sending full context.
//!
//! History is never verified against the backend session — the session owns
//! it (pass-through semantics, per design). The one concurrency rule is
//! per-conversation single flight: a new request displaces the in-flight
//! turn (cancel, then prompt) instead of queueing behind it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::{watch, Notify};

use super::turn::TurnEventForwarder;

/// Idle this long and the conversation's agent process is shut down; the
/// native session stays resumable and the next turn respawns with resume.
const IDLE_AGENT_SHUTDOWN: Duration = Duration::from_secs(5 * 60);
/// Idle this long and the conversation is forgotten entirely.
const IDLE_CONVERSATION_DROP: Duration = Duration::from_secs(2 * 60 * 60);
/// Hard cap on tracked conversations; beyond it the oldest idle ones go.
const MAX_CONVERSATIONS: usize = 32;
const EVICTION_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// How long a displacement waits for the cancelled turn to wind down before
/// force-replacing the agent process.
pub(super) const DISPLACE_GRACE: Duration = Duration::from_secs(35);

pub(super) struct ConversationRegistry {
    inner: Mutex<RegistryState>,
}

#[derive(Default)]
struct RegistryState {
    by_key: HashMap<String, Arc<Conversation>>,
    /// Latest response id of each Responses-protocol conversation. Only the
    /// latest is continuable — sessions cannot rewind, so branching from an
    /// older response is refused rather than silently mis-answered.
    by_response_id: HashMap<String, String>,
}

pub(super) enum ResponseLookup {
    Found(Arc<Conversation>),
    /// The id was never seen (or the daemon restarted since).
    NotFound,
    /// The id exists but a newer response supersedes it.
    Superseded,
}

pub(super) fn registry() -> &'static ConversationRegistry {
    static REGISTRY: OnceLock<ConversationRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        tokio::spawn(async {
            let mut interval = tokio::time::interval(EVICTION_SWEEP_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                registry().sweep().await;
            }
        });
        ConversationRegistry {
            inner: Mutex::new(RegistryState::default()),
        }
    })
}

impl ConversationRegistry {
    /// Find or create the conversation behind an explicit client key.
    pub(super) fn resolve_keyed(
        &self,
        client_key: &str,
        agent_id: &str,
        profile_id: &str,
        workspace: &PathBuf,
    ) -> Arc<Conversation> {
        let key = format!(
            "hdr\u{0}{agent_id}\u{0}{profile_id}\u{0}{}\u{0}{client_key}",
            workspace.to_string_lossy()
        );
        let mut inner = self.lock();
        if let Some(existing) = inner.by_key.get(&key) {
            return Arc::clone(existing);
        }
        let conversation = Conversation::new(key.clone(), agent_id, profile_id, workspace.clone());
        inner.by_key.insert(key, Arc::clone(&conversation));
        conversation
    }

    /// Register a fresh Responses-protocol conversation reachable through the
    /// response id its first turn is about to mint.
    pub(super) fn create_for_response(
        &self,
        agent_id: &str,
        profile_id: &str,
        workspace: &PathBuf,
        response_id: &str,
    ) -> Arc<Conversation> {
        let key = format!("resp\u{0}{response_id}");
        let conversation = Conversation::new(key.clone(), agent_id, profile_id, workspace.clone());
        let mut inner = self.lock();
        inner.by_key.insert(key.clone(), Arc::clone(&conversation));
        inner.by_response_id.insert(response_id.to_string(), key);
        conversation
    }

    pub(super) fn lookup_response(&self, response_id: &str) -> ResponseLookup {
        let inner = self.lock();
        let Some(key) = inner.by_response_id.get(response_id) else {
            return ResponseLookup::NotFound;
        };
        let Some(conversation) = inner.by_key.get(key) else {
            return ResponseLookup::NotFound;
        };
        if conversation.latest_response_id().as_deref() != Some(response_id) {
            return ResponseLookup::Superseded;
        }
        ResponseLookup::Found(Arc::clone(conversation))
    }

    /// Advance a conversation's chain to the response id its next turn mints.
    /// The previous id stops being continuable.
    pub(super) fn advance_response_id(&self, conversation: &Arc<Conversation>, response_id: &str) {
        let mut inner = self.lock();
        if let Some(previous) = conversation.swap_latest_response_id(response_id) {
            inner.by_response_id.remove(&previous);
        }
        inner
            .by_response_id
            .insert(response_id.to_string(), conversation.key.clone());
    }

    async fn sweep(&self) {
        let (to_shutdown, dropped) = {
            let mut inner = self.lock();
            let now = Instant::now();
            let mut to_shutdown = Vec::new();
            let mut dropped = Vec::new();
            let mut entries: Vec<(String, Arc<Conversation>)> = inner
                .by_key
                .iter()
                .map(|(key, conversation)| (key.clone(), Arc::clone(conversation)))
                .collect();
            entries.sort_by_key(|(_, conversation)| conversation.last_used());
            let overflow = entries.len().saturating_sub(MAX_CONVERSATIONS);
            for (index, (key, conversation)) in entries.into_iter().enumerate() {
                if conversation.turn_in_flight() {
                    continue;
                }
                let idle = now.saturating_duration_since(conversation.last_used());
                if idle >= IDLE_CONVERSATION_DROP || index < overflow {
                    inner.by_key.remove(&key);
                    if let Some(response_id) = conversation.latest_response_id() {
                        inner.by_response_id.remove(&response_id);
                    }
                    dropped.push(conversation);
                } else if idle >= IDLE_AGENT_SHUTDOWN {
                    to_shutdown.push(conversation);
                }
            }
            (to_shutdown, dropped)
        };
        for conversation in to_shutdown.into_iter().chain(dropped) {
            conversation.shutdown_agent_if_idle().await;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RegistryState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// One client conversation bound to one backend agent session.
pub(super) struct Conversation {
    pub(super) key: String,
    pub(super) agent_id: String,
    pub(super) profile_id: String,
    pub(super) workspace: PathBuf,
    pub(super) route: common::routing::RouteKey,
    state: Mutex<ConversationState>,
}

#[derive(Default)]
struct ConversationState {
    agent: Option<AgentGeneration>,
    session_id: Option<String>,
    instructions_fingerprint: Option<u64>,
    latest_response_id: Option<String>,
    last_used: Option<Instant>,
    in_flight: Option<InFlightTurn>,
}

/// One live agent process together with the event forwarder installed at its
/// spawn; both are replaced as a unit.
#[derive(Clone)]
pub(super) struct AgentGeneration {
    pub(super) agent: Arc<common::agent::Agent>,
    pub(super) forwarder: Arc<TurnEventForwarder>,
}

struct InFlightTurn {
    cancel: watch::Sender<bool>,
    finished: Arc<Notify>,
    finished_flag: Arc<AtomicBool>,
}

/// Registration of a running turn; dropping it marks the turn finished so a
/// displacing request stops waiting.
pub(super) struct TurnGuard {
    conversation: Arc<Conversation>,
    finished: Arc<Notify>,
    finished_flag: Arc<AtomicBool>,
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.finished_flag.store(true, Ordering::SeqCst);
        self.finished.notify_waiters();
        let mut state = self.conversation.lock();
        state.in_flight = None;
        state.last_used = Some(Instant::now());
    }
}

impl Conversation {
    fn new(key: String, agent_id: &str, profile_id: &str, workspace: PathBuf) -> Arc<Self> {
        let route = common::routing::RouteKey::new(
            super::LOCAL_AGENT_CHANNEL_KIND,
            format!("conv_{}", stable_hash(&key)),
        );
        Arc::new(Self {
            key,
            agent_id: agent_id.to_string(),
            profile_id: profile_id.to_string(),
            workspace,
            route,
            state: Mutex::new(ConversationState::default()),
        })
    }

    /// Displace whatever turn is running, then register the caller as the
    /// in-flight turn. Returns the guard plus a displacement-cancel receiver
    /// the new turn must honor itself.
    pub(super) async fn begin_turn(self: &Arc<Self>) -> (TurnGuard, watch::Receiver<bool>) {
        loop {
            let displaced = {
                let mut state = self.lock();
                match state.in_flight.take() {
                    Some(previous) => previous,
                    None => {
                        let (cancel_tx, cancel_rx) = watch::channel(false);
                        let finished = Arc::new(Notify::new());
                        let finished_flag = Arc::new(AtomicBool::new(false));
                        state.in_flight = Some(InFlightTurn {
                            cancel: cancel_tx,
                            finished: Arc::clone(&finished),
                            finished_flag: Arc::clone(&finished_flag),
                        });
                        state.last_used = Some(Instant::now());
                        let guard = TurnGuard {
                            conversation: Arc::clone(self),
                            finished,
                            finished_flag,
                        };
                        return (guard, cancel_rx);
                    }
                }
            };
            // The client retried or moved on: the running turn loses. Cancel
            // it and wait for it to wind down; a turn that will not die takes
            // its agent generation with it.
            let _ = displaced.cancel.send(true);
            let deadline = tokio::time::Instant::now() + DISPLACE_GRACE;
            loop {
                if displaced.finished_flag.load(Ordering::SeqCst) {
                    break;
                }
                if tokio::time::timeout_at(deadline, displaced.finished.notified())
                    .await
                    .is_err()
                {
                    self.force_replace_agent().await;
                    break;
                }
            }
        }
    }

    /// The live agent generation, if the process is still up.
    pub(super) fn live_agent(&self) -> Option<AgentGeneration> {
        let state = self.lock();
        state
            .agent
            .as_ref()
            .filter(|generation| generation.agent.is_live())
            .cloned()
    }

    pub(super) fn session_id(&self) -> Option<String> {
        self.lock().session_id.clone()
    }

    pub(super) fn set_agent(&self, generation: AgentGeneration) {
        self.lock().agent = Some(generation);
    }

    /// Drop the stored generation when its process died mid-turn so the next
    /// turn respawns instead of erroring on a dead handle.
    pub(super) fn clear_dead_agent(&self) {
        let mut state = self.lock();
        if state
            .agent
            .as_ref()
            .is_some_and(|generation| !generation.agent.is_live())
        {
            state.agent = None;
        }
    }

    pub(super) fn set_session_id(&self, session_id: Option<String>) {
        self.lock().session_id = session_id;
    }

    /// Whether the client's instructions changed since this conversation was
    /// seeded. A change means the old session no longer matches what the
    /// client believes it is talking to, so the caller reseeds.
    pub(super) fn instructions_changed(&self, fingerprint: u64) -> bool {
        let state = self.lock();
        matches!(state.instructions_fingerprint, Some(previous) if previous != fingerprint)
    }

    pub(super) fn set_instructions_fingerprint(&self, fingerprint: u64) {
        self.lock().instructions_fingerprint = Some(fingerprint);
    }

    /// Drop the current agent generation and session binding so the next
    /// turn starts a fresh session (used when instructions change).
    pub(super) async fn reset_session(&self) {
        let generation = {
            let mut state = self.lock();
            state.session_id = None;
            state.instructions_fingerprint = None;
            state.agent.take()
        };
        if let Some(generation) = generation {
            generation.agent.shutdown().await;
        }
    }

    async fn force_replace_agent(&self) {
        let generation = {
            let mut state = self.lock();
            state.in_flight = None;
            state.agent.take()
        };
        if let Some(generation) = generation {
            generation.agent.shutdown().await;
        }
    }

    async fn shutdown_agent_if_idle(&self) {
        let generation = {
            let mut state = self.lock();
            if state.in_flight.is_some() {
                return;
            }
            state.agent.take()
        };
        if let Some(generation) = generation {
            generation.agent.shutdown().await;
        }
    }

    fn turn_in_flight(&self) -> bool {
        self.lock().in_flight.is_some()
    }

    fn last_used(&self) -> Instant {
        self.lock().last_used.unwrap_or_else(Instant::now)
    }

    fn latest_response_id(&self) -> Option<String> {
        self.lock().latest_response_id.clone()
    }

    fn swap_latest_response_id(&self, response_id: &str) -> Option<String> {
        let mut state = self.lock();
        state.latest_response_id.replace(response_id.to_string())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ConversationState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub(super) fn instructions_fingerprint(instructions: &[va_ai_api_bridge::ContentBlock]) -> u64 {
    let serialized = serde_json::to_string(instructions).unwrap_or_default();
    stable_hash(&serialized)
}

fn stable_hash(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> ConversationRegistry {
        ConversationRegistry {
            inner: Mutex::new(RegistryState::default()),
        }
    }

    #[test]
    fn keyed_conversations_are_scoped_by_identity() {
        let registry = test_registry();
        let workspace = PathBuf::from("/tmp/project");
        let first = registry.resolve_keyed("chat-1", "claude", "deepseek", &workspace);
        let same = registry.resolve_keyed("chat-1", "claude", "deepseek", &workspace);
        assert!(Arc::ptr_eq(&first, &same));

        let other_key = registry.resolve_keyed("chat-2", "claude", "deepseek", &workspace);
        assert!(!Arc::ptr_eq(&first, &other_key));
        let other_agent = registry.resolve_keyed("chat-1", "codex", "deepseek", &workspace);
        assert!(!Arc::ptr_eq(&first, &other_agent));
        let other_workspace =
            registry.resolve_keyed("chat-1", "claude", "deepseek", &PathBuf::from("/tmp/other"));
        assert!(!Arc::ptr_eq(&first, &other_workspace));
    }

    #[test]
    fn response_chain_only_continues_from_the_latest() {
        let registry = test_registry();
        let workspace = PathBuf::from("/tmp/project");
        let conversation = registry.create_for_response("claude", "deepseek", &workspace, "resp_1");
        registry.advance_response_id(&conversation, "resp_1");

        let ResponseLookup::Found(found) = registry.lookup_response("resp_1") else {
            panic!("latest response id continues the conversation");
        };
        assert!(Arc::ptr_eq(&found, &conversation));

        registry.advance_response_id(&conversation, "resp_2");
        assert!(matches!(
            registry.lookup_response("resp_1"),
            ResponseLookup::NotFound
        ));
        assert!(matches!(
            registry.lookup_response("resp_2"),
            ResponseLookup::Found(_)
        ));
        assert!(matches!(
            registry.lookup_response("resp_unknown"),
            ResponseLookup::NotFound
        ));
    }

    #[test]
    fn instructions_change_is_detected_once_seeded() {
        let registry = test_registry();
        let conversation =
            registry.resolve_keyed("chat-1", "claude", "deepseek", &PathBuf::from("/tmp/p"));

        // Never seeded: nothing counts as changed.
        assert!(!conversation.instructions_changed(42));
        conversation.set_instructions_fingerprint(42);
        assert!(!conversation.instructions_changed(42));
        assert!(conversation.instructions_changed(43));
    }

    #[tokio::test]
    async fn a_new_turn_displaces_the_running_one() {
        let registry = test_registry();
        let conversation =
            registry.resolve_keyed("chat-1", "claude", "deepseek", &PathBuf::from("/tmp/p"));

        let (first_guard, mut first_cancel) = conversation.begin_turn().await;
        assert!(!*first_cancel.borrow());

        let second = {
            let conversation = Arc::clone(&conversation);
            tokio::spawn(async move { conversation.begin_turn().await })
        };
        // The displacement signal reaches the running turn...
        first_cancel
            .changed()
            .await
            .expect("displacement signal arrives");
        assert!(*first_cancel.borrow());
        // ...and the new turn waits until the old one winds down.
        assert!(!second.is_finished());
        drop(first_guard);
        let (second_guard, second_cancel) = second.await.expect("second turn begins");
        assert!(!*second_cancel.borrow());
        drop(second_guard);
    }
}
