use std::collections::BTreeMap;

use super::common::LaunchPlan;
use crate::profiles::terminal;

pub(super) fn spawn_if_enabled(plan: &LaunchPlan) -> Option<anyhow::Result<()>> {
    if std::env::var("VIBEAROUND_USE_VA_LAUNCHER_LIB")
        .ok()
        .as_deref()
        != Some("1")
    {
        return None;
    }
    let result = va_launcher::launch(input_from_plan(plan)).map(|_| ());
    Some(result)
}

fn input_from_plan(plan: &LaunchPlan) -> va_launcher::NativeLaunchInput {
    va_launcher::NativeLaunchInput {
        schema_version: 1,
        agent: String::new(),
        profile_id: None,
        launch_target: None,
        workspace: Some(plan.workspace.clone()),
        session_id: None,
        terminal: Some(terminal_choice_for_va_launch(terminal::read_preference())),
        command: Some(plan.command.clone()),
        executable_path: plan.windows_executable_path.clone(),
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

    #[test]
    fn maps_desktop_launch_plan_to_va_launch_input() {
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

        let input = input_from_plan(&plan);

        assert_eq!(input.schema_version, 1);
        assert_eq!(input.workspace, Some(PathBuf::from("/tmp/work")));
        assert_eq!(input.command.as_deref(), Some("codex"));
        assert_eq!(
            input.executable_path,
            Some(PathBuf::from("C:/Codex/Codex.exe"))
        );
        assert_eq!(input.window_label.as_deref(), Some("Codex profile"));
        assert_eq!(input.env["OPENAI_API_KEY"], "secret");
        assert_eq!(input.args.native, vec!["resume", "abc"]);
        assert_eq!(input.cleanup_paths, vec![PathBuf::from("/tmp/cleanup")]);
        assert_eq!(input.macos_app_probe.as_deref(), Some("Codex"));
        assert_eq!(input.windows_process_probe.as_deref(), Some("Codex"));
    }

    #[test]
    fn maps_terminal_ids_to_va_launch_ids() {
        assert_eq!(
            terminal_choice_for_va_launch(terminal::TerminalChoice::XfceTerminal).id(),
            terminal::TerminalChoice::XfceTerminal.id()
        );
    }
}
