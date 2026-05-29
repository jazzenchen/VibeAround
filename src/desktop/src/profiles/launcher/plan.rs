//! Launch plan builder.
//!
//! This module decides what should be launched. Platform modules only execute
//! the final plan, which keeps terminal-specific code away from profile,
//! bridge, and resume routing decisions.

use ::common::{profiles, resources};
use anyhow::anyhow;
use profiles::ProfileDef;

use super::common::LaunchPlan;
use super::{bridge, codex};

enum LaunchTarget<'a> {
    Profile {
        profile: &'a ProfileDef,
        launch_target: &'a str,
    },
    Direct {
        agent_id: &'a str,
    },
}

pub(super) struct LaunchPlanBuilder<'a> {
    launch_id: String,
    target: Option<LaunchTarget<'a>>,
    session_id: Option<&'a str>,
}

impl<'a> LaunchPlanBuilder<'a> {
    pub(super) fn new() -> Self {
        Self {
            launch_id: uuid::Uuid::new_v4().to_string(),
            target: None,
            session_id: None,
        }
    }

    pub(super) fn profile(mut self, profile: &'a ProfileDef, launch_target: &'a str) -> Self {
        self.target = Some(LaunchTarget::Profile {
            profile,
            launch_target,
        });
        self
    }

    pub(super) fn direct(mut self, agent_id: &'a str) -> Self {
        self.target = Some(LaunchTarget::Direct { agent_id });
        self
    }

    pub(super) fn resume(mut self, session_id: &'a str) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub(super) fn build(self) -> anyhow::Result<LaunchPlan> {
        match self
            .target
            .as_ref()
            .ok_or_else(|| anyhow!("launch target is required"))?
        {
            LaunchTarget::Profile {
                profile,
                launch_target,
            } => self.build_profile_plan(profile, launch_target),
            LaunchTarget::Direct { agent_id } => self.build_direct_plan(agent_id),
        }
    }

    fn build_profile_plan(
        &self,
        profile: &ProfileDef,
        launch_target: &str,
    ) -> anyhow::Result<LaunchPlan> {
        let mut rendered = bridge::render_for_launch(profile, launch_target, &self.launch_id)?;
        codex::apply_session_hooks(profile, launch_target, &self.launch_id, &mut rendered)?;

        match self.session_id {
            Some(session_id) => self.build_rendered_profile_resume_plan(
                profile,
                launch_target,
                rendered,
                session_id,
            ),
            None => self.build_rendered_profile_plan(profile, launch_target, rendered),
        }
    }

    fn build_direct_plan(&self, agent_id: &str) -> anyhow::Result<LaunchPlan> {
        let agent = resources::agent_by_id(agent_id)
            .ok_or_else(|| anyhow!("agent '{}' not found in agents.json", agent_id))?;
        let workspace = crate::profiles::resolve_launch_workspace(agent_id)?;

        let Some(session_id) = self.session_id else {
            return Ok(LaunchPlan {
                env: Vec::new(),
                command: agent.pty.command.clone(),
                args: Vec::new(),
                window_label: format!("{} (direct)", agent.display_name),
                workspace,
            });
        };

        let (command, args) = resume_command_for_agent(agent_id, session_id)?;
        Ok(LaunchPlan {
            env: Vec::new(),
            command,
            args,
            window_label: format!("{} (resume)", agent.display_name),
            workspace,
        })
    }

    fn build_rendered_profile_plan(
        &self,
        profile: &ProfileDef,
        launch_target: &str,
        rendered: profiles::render::RenderedProfile,
    ) -> anyhow::Result<LaunchPlan> {
        let command_args = rendered.command_args.clone();
        let env = materialized_profile_env(profile, launch_target, &self.launch_id, rendered)?;

        let agent_id = profiles::runtime::agent_id_for(launch_target)?;
        let agent = resources::agent_by_id(agent_id)
            .ok_or_else(|| anyhow!("agent '{}' not found in agents.json", agent_id))?;
        let workspace = crate::profiles::resolve_launch_workspace(agent_id)?;

        Ok(LaunchPlan {
            env,
            command: agent.pty.command.clone(),
            args: command_args,
            window_label: profile.label.clone(),
            workspace,
        })
    }

    fn build_rendered_profile_resume_plan(
        &self,
        profile: &ProfileDef,
        launch_target: &str,
        rendered: profiles::render::RenderedProfile,
        session_id: &str,
    ) -> anyhow::Result<LaunchPlan> {
        let env =
            materialized_profile_env(profile, launch_target, &self.launch_id, rendered.clone())?;

        let agent_id = profiles::runtime::agent_id_for(launch_target)?;
        let workspace = crate::profiles::resolve_launch_workspace(agent_id)?;
        let (command, mut args) = resume_command_for_agent(agent_id, session_id)?;
        if !rendered.command_args.is_empty() {
            let mut profile_args = rendered.command_args.clone();
            profile_args.extend(args);
            args = profile_args;
        }

        Ok(LaunchPlan {
            env,
            command,
            args,
            window_label: format!("{} (resume)", profile.label),
            workspace,
        })
    }
}

fn materialized_profile_env(
    profile: &ProfileDef,
    launch_target: &str,
    launch_id: &str,
    rendered: profiles::render::RenderedProfile,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut env = profiles::runtime::materialize_env(&profile.id, rendered)?;
    if !bridge::launch_uses_local_bridge(profile, launch_target)? {
        profiles::runtime::append_settings_proxy_env(profile, &mut env)?;
    }
    env.push(("VIBEAROUND_LAUNCH_ID".to_string(), launch_id.to_string()));
    env.push(("VIBEAROUND_PROFILE_ID".to_string(), profile.id.clone()));
    env.push((
        "VIBEAROUND_LAUNCH_TARGET".to_string(),
        launch_target.to_string(),
    ));
    Ok(env)
}

fn resume_command_for_agent(
    agent_id: &str,
    session_id: &str,
) -> anyhow::Result<(String, Vec<String>)> {
    let command = match agent_id {
        "claude" => (
            "claude".to_string(),
            vec![
                "--resume".to_string(),
                session_id.to_string(),
                "--permission-mode".to_string(),
                "acceptEdits".to_string(),
            ],
        ),
        "codex" => (
            "codex".to_string(),
            vec!["resume".to_string(), session_id.to_string()],
        ),
        "pi" => (
            "pi".to_string(),
            vec!["--session".to_string(), session_id.to_string()],
        ),
        "gemini" => (
            "gemini".to_string(),
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        "opencode" => (
            "opencode".to_string(),
            vec!["--session".to_string(), session_id.to_string()],
        ),
        "cursor" => (
            "cursor-agent".to_string(),
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        "qwen-code" => (
            "qwen".to_string(),
            vec!["--resume".to_string(), session_id.to_string()],
        ),
        other => {
            return Err(anyhow!(
                "resume launch is not supported for agent '{}'",
                other
            ))
        }
    };
    Ok(command)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ::common::profiles::schema::{ApiTypeOverrides, AuthMode, ProfileDef, ProviderSettings};

    use super::*;

    impl<'a> LaunchPlanBuilder<'a> {
        fn with_launch_id(launch_id: &str) -> Self {
            Self {
                launch_id: launch_id.to_string(),
                target: None,
                session_id: None,
            }
        }
    }

    fn minimax_anthropic_profile() -> ProfileDef {
        ProfileDef {
            id: "minimax-test".to_string(),
            label: "MiniMax Test".to_string(),
            provider: "minimax".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["anthropic".to_string()],
            credentials: [("api_key".to_string(), "test-key".to_string())]
                .into_iter()
                .collect(),
            overrides: [(
                "anthropic".to_string(),
                ApiTypeOverrides {
                    model: Some("MiniMax-M2.7".to_string()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
        }
    }

    #[test]
    fn direct_launch_plan_has_no_profile_env() {
        let plan = LaunchPlanBuilder::with_launch_id("launch-123")
            .direct("claude")
            .build()
            .expect("direct plan");

        assert_eq!(plan.command, "claude code --permission-mode acceptEdits");
        assert!(plan.args.is_empty());
        assert!(plan.env.is_empty());
        assert_eq!(plan.window_label, "Claude Code (direct)");
    }

    #[test]
    fn direct_resume_plan_uses_agent_resume_command() {
        let plan = LaunchPlanBuilder::with_launch_id("launch-123")
            .direct("claude")
            .resume("session-456")
            .build()
            .expect("direct resume plan");

        assert_eq!(plan.command, "claude");
        assert_eq!(
            plan.args,
            vec![
                "--resume".to_string(),
                "session-456".to_string(),
                "--permission-mode".to_string(),
                "acceptEdits".to_string(),
            ]
        );
        assert_eq!(plan.window_label, "Claude Code (resume)");
    }

    #[test]
    fn direct_resume_plan_supports_pi_sessions() {
        let plan = LaunchPlanBuilder::with_launch_id("launch-123")
            .direct("pi")
            .resume("019e57e0")
            .build()
            .expect("pi direct resume plan");

        assert_eq!(plan.command, "pi");
        assert_eq!(
            plan.args,
            vec!["--session".to_string(), "019e57e0".to_string()]
        );
        assert_eq!(plan.window_label, "Pi (resume)");
    }

    #[test]
    fn profile_launch_plan_adds_vibearound_identity_env() {
        let profile = minimax_anthropic_profile();
        let plan = LaunchPlanBuilder::with_launch_id("launch-123")
            .profile(&profile, "claude")
            .build()
            .expect("profile plan");

        assert_eq!(plan.command, "claude code --permission-mode acceptEdits");
        assert_eq!(plan.window_label, "MiniMax Test");
        assert!(plan
            .env
            .contains(&("VIBEAROUND_LAUNCH_ID".to_string(), "launch-123".to_string())));
        assert!(plan.env.contains(&(
            "VIBEAROUND_PROFILE_ID".to_string(),
            "minimax-test".to_string()
        )));
        assert!(plan
            .env
            .contains(&("VIBEAROUND_LAUNCH_TARGET".to_string(), "claude".to_string())));
    }

    #[test]
    fn pi_profile_launch_plan_includes_extension_args() {
        let profile = minimax_anthropic_profile();
        let plan = LaunchPlanBuilder::with_launch_id("launch-123")
            .profile(&profile, "pi")
            .build()
            .expect("pi profile plan");

        assert_eq!(plan.command, "pi");
        assert!(plan
            .args
            .windows(2)
            .any(|args| args[0] == "--provider" && args[1] == "vibearound-minimax-test-anthropic"));
        assert!(plan
            .args
            .windows(2)
            .any(|args| args[0] == "--model" && args[1] == "MiniMax-M2.7"));
        assert!(plan
            .env
            .contains(&("VIBEAROUND_PI_API_KEY".to_string(), "test-key".to_string())));
        assert!(plan
            .env
            .contains(&("VIBEAROUND_LAUNCH_TARGET".to_string(), "pi".to_string())));
    }

    #[test]
    fn pi_profile_resume_keeps_profile_extension_args() {
        let mut profile = minimax_anthropic_profile();
        profile.id = "minimax-resume-test".to_string();
        let plan = LaunchPlanBuilder::with_launch_id("launch-123")
            .profile(&profile, "pi")
            .resume("019e57e0")
            .build()
            .expect("pi profile resume plan");

        assert_eq!(plan.command, "pi");
        assert!(plan
            .args
            .windows(2)
            .any(|args| args[0] == "--provider"
                && args[1] == "vibearound-minimax-resume-test-anthropic"));
        assert!(plan.args.windows(2).any(|args| args[0] == "--session"));
        assert_eq!(plan.args.last().map(String::as_str), Some("019e57e0"));
        assert!(plan
            .env
            .contains(&("VIBEAROUND_PI_API_KEY".to_string(), "test-key".to_string())));
    }
}
