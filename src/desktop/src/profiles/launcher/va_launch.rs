use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use super::common::LaunchPlan;
use crate::profiles::terminal;
use anyhow::{bail, Context};
use common::profiles::ProfileDef;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LaunchContext {
    agent_id: String,
    profile_id: Option<String>,
    launch_target: Option<String>,
    session_id: Option<String>,
}

impl LaunchContext {
    pub(super) fn profile(
        profile: &ProfileDef,
        launch_target: &str,
        session_id: Option<&str>,
    ) -> Self {
        Self {
            agent_id: launch_target.to_string(),
            profile_id: Some(profile.id.clone()),
            launch_target: Some(launch_target.to_string()),
            session_id: session_id.map(ToString::to_string),
        }
    }

    pub(super) fn direct(agent_id: &str, session_id: Option<&str>) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            profile_id: None,
            launch_target: None,
            session_id: session_id.map(ToString::to_string),
        }
    }
}

pub(super) fn spawn(plan: &LaunchPlan, context: &LaunchContext) -> anyhow::Result<()> {
    let profile = profile_from_plan(plan, context);
    let profile_path = write_profile_file(&profile)?;
    let launcher = resolve_va_launch_binary()?;
    let status = Command::new(&launcher)
        .arg("--profile-path")
        .arg(&profile_path)
        .status()
        .with_context(|| format!("invoke va-launch at {}", launcher.display()));
    let _ = std::fs::remove_file(&profile_path);
    let status = status?;
    if !status.success() {
        bail!("va-launch failed with exit {:?}", status.code());
    }
    Ok(())
}

fn write_profile_file(profile: &va_launcher::LaunchProfile) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "vibearound-va-launch-profile-{}.json",
        uuid::Uuid::new_v4()
    ));
    let body = serde_json::to_string(profile).context("serialize va-launch profile")?;
    std::fs::write(&path, body).with_context(|| format!("write {}", path.display()))?;
    common::auth::set_owner_only(&path).ok();
    Ok(path)
}

fn resolve_va_launch_binary() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("VIBEAROUND_VA_LAUNCH_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        bail!("VIBEAROUND_VA_LAUNCH_BIN is not a file: {}", path.display());
    }

    let binary = if cfg!(target_os = "windows") {
        "va-launch.exe"
    } else {
        "va-launch"
    };

    let sibling = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(binary)));
    if let Some(path) = sibling {
        if path.is_file() {
            return Ok(path);
        }
    }

    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("target");
    for profile in ["debug", "release"] {
        let path = target_dir.join(profile).join(binary);
        if path.is_file() {
            return Ok(path);
        }
    }

    bail!("va-launch binary not found; build va-launcher or set VIBEAROUND_VA_LAUNCH_BIN")
}

fn profile_from_plan(plan: &LaunchPlan, context: &LaunchContext) -> va_launcher::LaunchProfile {
    va_launcher::LaunchProfile {
        schema_version: Some(1),
        id: context.profile_id.clone(),
        agent: context.agent_id.clone(),
        profile_id: context.profile_id.clone(),
        launch_target: context.launch_target.clone(),
        workspace: Some(plan.workspace.clone()),
        session_id: context.session_id.clone(),
        terminal: Some(terminal_choice_for_va_launch(terminal::read_preference())),
        command: Some(plan.command.clone()),
        executable_path: None,
        windows_executable_path: plan.windows_executable_path.clone(),
        window_label: Some(plan.window_label.clone()),
        env: plan.env.iter().cloned().collect::<BTreeMap<_, _>>(),
        args: va_launcher::NativeLaunchArgs {
            native: plan.args.clone(),
        },
        cleanup_paths: plan.cleanup_paths.clone(),
        macos_app_probe: plan.macos_app_probe.clone(),
        windows_process_probe: plan.windows_process_probe.clone(),
    }
}

fn terminal_choice_for_va_launch(choice: terminal::TerminalChoice) -> va_launcher::TerminalChoice {
    match choice {
        terminal::TerminalChoice::Terminal => va_launcher::TerminalChoice::Terminal,
        terminal::TerminalChoice::Iterm2 => va_launcher::TerminalChoice::Iterm2,
        terminal::TerminalChoice::PowerShell => va_launcher::TerminalChoice::PowerShell,
        terminal::TerminalChoice::SystemTerminal => va_launcher::TerminalChoice::SystemTerminal,
        terminal::TerminalChoice::GnomeTerminal => va_launcher::TerminalChoice::GnomeTerminal,
        terminal::TerminalChoice::Konsole => va_launcher::TerminalChoice::Konsole,
        terminal::TerminalChoice::XfceTerminal => va_launcher::TerminalChoice::XfceTerminal,
        terminal::TerminalChoice::Xterm => va_launcher::TerminalChoice::Xterm,
        terminal::TerminalChoice::Kitty => va_launcher::TerminalChoice::Kitty,
        terminal::TerminalChoice::Alacritty => va_launcher::TerminalChoice::Alacritty,
        terminal::TerminalChoice::WezTerm => va_launcher::TerminalChoice::WezTerm,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use common::profiles::schema::{AuthMode, ProfileDef, ProviderSettings};

    fn profile(id: &str) -> ProfileDef {
        ProfileDef {
            id: id.to_string(),
            label: "Test profile".to_string(),
            provider: "openai".to_string(),
            auth_mode: AuthMode::ApiKey,
            api_types: vec!["openai_responses".to_string()],
            credentials: Default::default(),
            overrides: Default::default(),
            use_settings_proxy: false,
            provider_settings: ProviderSettings::default(),
            connections: Default::default(),
        }
    }

    #[test]
    fn maps_desktop_launch_plan_to_va_launch_profile() {
        let plan = LaunchPlan {
            env: vec![("OPENAI_API_KEY".to_string(), "secret".to_string())],
            command: "codex".to_string(),
            args: vec!["resume".to_string(), "abc".to_string()],
            cleanup_paths: vec![PathBuf::from("/tmp/cleanup")],
            window_label: "Codex profile".to_string(),
            workspace: PathBuf::from("/tmp/work"),
            macos_app_probe: Some("Codex".to_string()),
            windows_process_probe: Some("Codex".to_string()),
            windows_executable_path: Some(PathBuf::from("C:/Codex/Codex.exe")),
        };

        let context = LaunchContext::profile(&profile("openai"), "codex", Some("session-123"));
        let profile = profile_from_plan(&plan, &context);

        assert_eq!(profile.schema_version, Some(1));
        assert_eq!(profile.id.as_deref(), Some("openai"));
        assert_eq!(profile.agent, "codex");
        assert_eq!(profile.profile_id.as_deref(), Some("openai"));
        assert_eq!(profile.launch_target.as_deref(), Some("codex"));
        assert_eq!(profile.session_id.as_deref(), Some("session-123"));
        assert_eq!(profile.workspace, Some(PathBuf::from("/tmp/work")));
        assert_eq!(profile.command.as_deref(), Some("codex"));
        assert_eq!(profile.executable_path, None);
        assert_eq!(
            profile.windows_executable_path,
            Some(PathBuf::from("C:/Codex/Codex.exe"))
        );
        assert_eq!(profile.window_label.as_deref(), Some("Codex profile"));
        assert_eq!(profile.env["OPENAI_API_KEY"], "secret");
        assert_eq!(profile.args.native, vec!["resume", "abc"]);
        assert_eq!(profile.cleanup_paths, vec![PathBuf::from("/tmp/cleanup")]);
        assert_eq!(profile.macos_app_probe.as_deref(), Some("Codex"));
        assert_eq!(profile.windows_process_probe.as_deref(), Some("Codex"));
    }

    #[test]
    fn maps_terminal_ids_to_va_launch_ids() {
        assert_eq!(
            terminal_choice_for_va_launch(terminal::TerminalChoice::XfceTerminal).id(),
            terminal::TerminalChoice::XfceTerminal.id()
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_launch_pushes_launch_profile_file_to_va_launch() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = env_test_lock().lock().expect("env test lock");
        let dir = std::env::temp_dir().join(format!(
            "vibearound-va-launch-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        let fake_bin = dir.join("va-launch");
        let capture = dir.join("capture.json");
        std::fs::write(
            &fake_bin,
            "#!/bin/sh\nif [ \"$1\" = \"--profile-path\" ]; then cp \"$2\" \"$VA_LAUNCH_CAPTURE\"; exit 0; fi\nexit 2\n",
        )
        .expect("write fake va-launch");
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o700))
            .expect("chmod fake va-launch");

        let previous_bin = std::env::var_os("VIBEAROUND_VA_LAUNCH_BIN");
        let previous_capture = std::env::var_os("VA_LAUNCH_CAPTURE");
        std::env::set_var("VIBEAROUND_VA_LAUNCH_BIN", &fake_bin);
        std::env::set_var("VA_LAUNCH_CAPTURE", &capture);

        let plan = LaunchPlan {
            env: vec![("OPENAI_API_KEY".to_string(), "secret".to_string())],
            command: "codex".to_string(),
            args: vec!["--model".to_string(), "gpt-5".to_string()],
            cleanup_paths: Vec::new(),
            window_label: "OpenAI".to_string(),
            workspace: PathBuf::from("/tmp/work"),
            macos_app_probe: None,
            windows_process_probe: None,
            windows_executable_path: None,
        };
        let context = LaunchContext::profile(&profile("openai"), "codex", None);

        spawn(&plan, &context).expect("spawn fake va-launch");

        let captured = std::fs::read_to_string(&capture).expect("read captured input");
        let value: serde_json::Value =
            serde_json::from_str(&captured).expect("parse captured JSON");
        restore_env("VIBEAROUND_VA_LAUNCH_BIN", previous_bin);
        restore_env("VA_LAUNCH_CAPTURE", previous_capture);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(value["agent"], "codex");
        assert_eq!(value["id"], "openai");
        assert_eq!(value["profileId"], "openai");
        assert_eq!(value["launchTarget"], "codex");
        assert_eq!(value["command"], "codex");
        assert_eq!(value["env"]["OPENAI_API_KEY"], "secret");
        assert_eq!(
            value["args"]["native"],
            serde_json::json!(["--model", "gpt-5"])
        );
    }

    #[test]
    fn explicit_va_launch_binary_must_exist() {
        let _guard = env_test_lock().lock().expect("env test lock");
        let previous = std::env::var_os("VIBEAROUND_VA_LAUNCH_BIN");
        std::env::set_var("VIBEAROUND_VA_LAUNCH_BIN", "/definitely/not/va-launch");

        let error = resolve_va_launch_binary().unwrap_err().to_string();

        restore_env("VIBEAROUND_VA_LAUNCH_BIN", previous);
        assert!(error.contains("VIBEAROUND_VA_LAUNCH_BIN is not a file"));
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn env_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }
}
