//! Shared agent availability resolution for onboarding, launch, and ACP.

use crate::agent_detection::{self, AgentCandidate, AgentDetection};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AgentScanPolicy {
    /// Use the persisted detection file only.
    CacheOnly,
    /// Always perform a fresh scan before selecting a candidate.
    Refresh,
    /// Use configured/cache first, then scan only when no candidate is selected.
    RefreshIfMissing,
    /// Trust an explicitly configured executable; otherwise perform a fresh scan.
    RefreshIfUnconfigured,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AgentCandidatePreference {
    ToolchainMode,
    SystemToolchain,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentAvailabilityRequest<'a> {
    pub scan_policy: AgentScanPolicy,
    pub toolchain_mode: &'a str,
    pub candidate_preference: AgentCandidatePreference,
    pub include_configured_version: bool,
}

#[derive(Debug, Clone)]
pub struct AgentAvailability {
    pub detection: AgentDetection,
    pub configured: Option<AgentCandidate>,
    pub selected: Option<AgentCandidate>,
    pub scanned: bool,
}

pub async fn resolve_agent_availability(
    agent_id: &str,
    request: AgentAvailabilityRequest<'_>,
) -> anyhow::Result<AgentAvailability> {
    let mut scanned = false;
    let mut detection = if request.scan_policy == AgentScanPolicy::Refresh {
        scanned = true;
        Some(agent_detection::scan_agent_and_persist(agent_id).await?)
    } else {
        cached_detection(agent_id)
    };

    let configured = if request.include_configured_version {
        agent_detection::configured_candidate_with_version(agent_id).await
    } else {
        agent_detection::configured_candidate(agent_id)
    };

    let mut detection = detection.take().unwrap_or_else(empty_detection);
    let mut selected = select_agent_candidate(
        agent_id,
        configured.clone(),
        &detection,
        request.toolchain_mode,
        request.candidate_preference,
    );

    let should_refresh_after_cache = match request.scan_policy {
        AgentScanPolicy::RefreshIfMissing => selected.is_none(),
        AgentScanPolicy::RefreshIfUnconfigured => configured.is_none(),
        AgentScanPolicy::CacheOnly | AgentScanPolicy::Refresh => false,
    };
    if should_refresh_after_cache {
        scanned = true;
        detection = agent_detection::scan_agent_and_persist(agent_id).await?;
        selected = select_agent_candidate(
            agent_id,
            configured.clone(),
            &detection,
            request.toolchain_mode,
            request.candidate_preference,
        );
    }

    Ok(AgentAvailability {
        detection,
        configured,
        selected,
        scanned,
    })
}

pub fn resolve_cached_agent_availability(
    agent_id: &str,
    toolchain_mode: &str,
    candidate_preference: AgentCandidatePreference,
) -> AgentAvailability {
    let detection = cached_detection(agent_id).unwrap_or_else(empty_detection);
    let configured = agent_detection::configured_candidate(agent_id);
    let selected = select_agent_candidate(
        agent_id,
        configured.clone(),
        &detection,
        toolchain_mode,
        candidate_preference,
    );

    AgentAvailability {
        detection,
        configured,
        selected,
        scanned: false,
    }
}

pub fn select_agent_candidate(
    agent_id: &str,
    configured: Option<AgentCandidate>,
    detection: &AgentDetection,
    toolchain_mode: &str,
    preference: AgentCandidatePreference,
) -> Option<AgentCandidate> {
    configured.or_else(|| match preference {
        AgentCandidatePreference::ToolchainMode => {
            agent_detection::preferred_startkit_candidate(agent_id, detection, toolchain_mode)
        }
        AgentCandidatePreference::SystemToolchain => detection.system_selected_candidate(),
    })
}

fn cached_detection(agent_id: &str) -> Option<AgentDetection> {
    agent_detection::read_detected_agents()?
        .agents
        .get(agent_id)
        .cloned()
}

fn empty_detection() -> AgentDetection {
    AgentDetection {
        default_candidate: None,
        system_selected: None,
        candidates: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{select_agent_candidate, AgentCandidatePreference};
    use crate::agent_detection::{AgentCandidate, AgentDetection};

    #[test]
    fn configured_candidate_wins_for_all_preferences() {
        let configured = test_candidate("/manual/codex", "manual_path", 0);
        let system = test_candidate("/usr/local/bin/codex", "npm_global", 1);
        let detection = detection_with(vec![system.clone()], Some(system));

        for preference in [
            AgentCandidatePreference::ToolchainMode,
            AgentCandidatePreference::SystemToolchain,
        ] {
            let selected = select_agent_candidate(
                "codex",
                Some(configured.clone()),
                &detection,
                "system",
                preference,
            )
            .expect("selected configured candidate");
            assert_eq!(selected.path, configured.path);
        }
    }

    #[test]
    fn toolchain_mode_can_select_managed_candidate() {
        let managed = test_candidate("/managed/bin/codex", "npm_managed", 10_000);
        let detection = detection_with(vec![managed.clone()], Some(managed.clone()));

        let selected = select_agent_candidate(
            "codex",
            None,
            &detection,
            "managed",
            AgentCandidatePreference::ToolchainMode,
        )
        .expect("selected managed candidate");

        assert_eq!(selected.path, managed.path);
    }

    #[test]
    fn system_preference_ignores_managed_candidate() {
        let managed = test_candidate("/managed/bin/codex", "npm_managed", 10_000);
        let system = test_candidate("/usr/local/bin/codex", "npm_global", 1);
        let detection = AgentDetection {
            default_candidate: Some(managed.clone()),
            system_selected: Some(system.clone()),
            candidates: vec![managed, system.clone()],
        };

        let selected = select_agent_candidate(
            "codex",
            None,
            &detection,
            "managed",
            AgentCandidatePreference::SystemToolchain,
        )
        .expect("selected system candidate");

        assert_eq!(selected.path, system.path);
    }

    fn detection_with(
        candidates: Vec<AgentCandidate>,
        selected: Option<AgentCandidate>,
    ) -> AgentDetection {
        AgentDetection {
            default_candidate: selected.clone(),
            system_selected: selected,
            candidates,
        }
    }

    fn test_candidate(path: &str, source: &str, rank: u32) -> AgentCandidate {
        AgentCandidate {
            path: path.to_string(),
            realpath: None,
            version: None,
            source: source.to_string(),
            source_label: source.to_string(),
            rank,
            is_user_default: rank == 0,
            package: None,
        }
    }
}
