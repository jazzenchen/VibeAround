use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

use crate::{resolve_configured_agent_executable, write_scanned_agent_executable};

pub fn resolve_executable_path(path: PathBuf) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(&path)
        .with_context(|| format!("agent executable does not exist: {}", path.display()))?;
    if !canonical.is_file() {
        bail!("agent executable is not a file: {}", canonical.display());
    }
    if !is_executable_file(&canonical) {
        bail!(
            "agent executable is not executable: {}",
            canonical.display()
        );
    }
    Ok(strip_windows_unc_prefix(canonical))
}

pub fn resolve_agent_launch_command(agent: &str, command: &str) -> anyhow::Result<String> {
    let program = first_command_word(command).context("launch command is empty")?;
    if is_shell_builtin_for_current_platform(&program) || is_app_launch_wrapper(&program) {
        validate_launch_command(command)?;
        return Ok(command.to_string());
    }

    if is_path_like_program(&program) {
        resolve_executable_path(PathBuf::from(&program))?;
        return Ok(command.to_string());
    }

    let command_uses_agent_executable = command_uses_default_agent_program(agent, &program);
    if command_uses_agent_executable {
        if let Some(configured) = resolve_configured_agent_executable(agent)? {
            let configured = resolve_executable_path(configured).with_context(|| {
                format!("configured executable for agent '{}' is invalid", agent)
            })?;
            if executable_matches_program(&configured, &program) {
                return replace_first_command_word(command, &configured);
            }
        }
    }

    if let Some(scanned) = find_program_in_path(&program) {
        if command_uses_agent_executable {
            write_scanned_agent_executable(agent, &scanned)?;
        }
        return replace_first_command_word(command, &scanned);
    }

    bail!("agent executable '{}' was not found in PATH", program)
}

pub fn validate_launch_command(command: &str) -> anyhow::Result<()> {
    let program = first_command_word(command).context("launch command is empty")?;
    if is_shell_builtin_for_current_platform(&program) {
        return Ok(());
    }
    if is_path_like_program(&program) {
        resolve_executable_path(PathBuf::from(&program))?;
        return Ok(());
    }
    if find_program_in_path(&program).is_some() {
        return Ok(());
    }
    bail!("agent executable '{}' was not found in PATH", program)
}

fn first_command_word(command: &str) -> Option<String> {
    first_command_word_span(command).map(|(word, _)| word)
}

fn first_command_word_span(command: &str) -> Option<(String, usize)> {
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote: Option<char> = None;
    let mut end = 0;

    while let Some(ch) = chars.next() {
        end += ch.len_utf8();
        match quote {
            Some(q) if ch == q => quote = None,
            Some('"') if ch == '\\' => {
                if matches!(chars.peek(), Some('"') | Some('\\')) {
                    let next = chars.next().expect("peeked next char");
                    end += next.len_utf8();
                    current.push(next);
                } else {
                    current.push(ch);
                }
            }
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    return Some((current, end));
                }
            }
            None => current.push(ch),
        }
    }

    if current.is_empty() {
        None
    } else {
        Some((current, end))
    }
}

fn is_shell_builtin_for_current_platform(program: &str) -> bool {
    cfg!(target_os = "windows") && matches!(program, "Start-Process")
}

fn is_app_launch_wrapper(program: &str) -> bool {
    cfg!(target_os = "macos") && program == "open"
}

fn is_path_like_program(program: &str) -> bool {
    Path::new(program).is_absolute() || program.contains('/') || program.contains('\\')
}

fn command_uses_default_agent_program(agent: &str, program: &str) -> bool {
    common::resources::agent_by_id(agent)
        .map(|agent| agent.launch_command_for_current_platform())
        .and_then(first_command_word)
        .is_some_and(|default_program| command_name_eq(&default_program, program))
}

fn executable_matches_program(path: &Path, program: &str) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| command_name_eq(name, program))
}

fn command_name_eq(left: &str, right: &str) -> bool {
    if cfg!(target_os = "windows") {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn replace_first_command_word(command: &str, path: &Path) -> anyhow::Result<String> {
    let (_, end) = first_command_word_span(command).context("launch command is empty")?;
    let suffix = command[end..].trim_start();
    let path = quote_command_word(path.to_string_lossy().as_ref());
    if suffix.is_empty() {
        Ok(path)
    } else {
        Ok(format!("{path} {suffix}"))
    }
}

fn quote_command_word(word: &str) -> String {
    if !word
        .chars()
        .any(|ch| ch.is_whitespace() || ch == '"' || ch == '\'')
    {
        return word.to_string();
    }
    format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\""))
}

fn find_program_in_path(program: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        for candidate in program_candidates(&dir, program) {
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn program_candidates(dir: &Path, program: &str) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let path = Path::new(program);
        if path.extension().is_some() {
            return vec![dir.join(program)];
        }
        let pathext = env::var_os("PATHEXT")
            .map(|value| {
                value
                    .to_string_lossy()
                    .split(';')
                    .filter(|ext| !ext.trim().is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![".EXE".to_string(), ".CMD".to_string(), ".BAT".to_string()]);
        let mut out = Vec::with_capacity(pathext.len() + 1);
        out.push(dir.join(program));
        out.extend(
            pathext
                .into_iter()
                .map(|ext| dir.join(format!("{program}{ext}"))),
        );
        out
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![dir.join(program)]
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn strip_windows_unc_prefix(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
                return PathBuf::from(rest.to_string());
            }
        }
        path
    }
    #[cfg(not(target_os = "windows"))]
    {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_command_from_path() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let fixture = PathFixture::with_command("codex");

        validate_launch_command("codex resume abc").expect("validate command");

        drop(fixture);
    }

    #[test]
    fn configured_executable_replaces_command_program_without_path_scan() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let configured_dir = dir.join("configured");
        std::fs::create_dir_all(&configured_dir).expect("create configured dir");
        let configured = write_command(&configured_dir, "codex");
        std::fs::write(
            dir.join("settings.json"),
            format!(
                r#"{{
  "launcher": {{
    "agents": {{
      "codex": {{
        "executable": {{
          "path": {},
          "source": "manual_path",
          "source_label": "Manual path",
          "rank": 0
        }}
      }}
    }}
  }}
}}"#,
                serde_json::to_string(configured.to_string_lossy().as_ref())
                    .expect("serialize configured path")
            ),
        )
        .expect("write settings config");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", "");

        let command = resolve_agent_launch_command("codex", "codex resume abc")
            .expect("resolve configured command");
        let expected = resolve_executable_path(configured.clone())
            .expect("resolve configured path")
            .to_string_lossy()
            .to_string();

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(command, format!("{expected} resume abc"));
    }

    #[test]
    fn unrelated_configured_executable_is_ignored() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let configured = write_command(&dir, "powershell");
        let bin_dir = dir.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let scanned = write_command(&bin_dir, "codex");
        std::fs::write(
            dir.join("settings.json"),
            format!(
                r#"{{
  "launcher": {{
    "agents": {{
      "codex": {{
        "executable": {{
          "path": {},
          "source": "path_scan",
          "source_label": "PATH scan",
          "rank": 4000
        }}
      }}
    }}
  }}
}}"#,
                serde_json::to_string(configured.to_string_lossy().as_ref())
                    .expect("serialize configured path")
            ),
        )
        .expect("write settings config");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", &bin_dir);

        let command = resolve_agent_launch_command("codex", "codex")
            .expect("resolve scanned command after ignoring unrelated config");

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(command, scanned.to_string_lossy());
    }

    #[test]
    fn path_scan_writes_agent_executable_config() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let bin_dir = dir.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let scanned = write_command(&bin_dir, "codex");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", &bin_dir);

        let command =
            resolve_agent_launch_command("codex", "codex").expect("resolve scanned command");
        let body =
            std::fs::read_to_string(dir.join("settings.json")).expect("read settings config");
        let value: serde_json::Value = serde_json::from_str(&body).expect("parse settings config");
        let agents_json_exists = dir.join("agents.json").exists();

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(command, scanned.to_string_lossy());
        assert_eq!(
            value["launcher"]["agents"]["codex"]["executable"]["path"].as_str(),
            Some(scanned.to_string_lossy().as_ref())
        );
        assert!(!agents_json_exists);
    }

    #[test]
    fn explicit_custom_command_does_not_write_agent_executable_config() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let bin_dir = dir.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let custom = write_command(&bin_dir, "powershell");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", &bin_dir);

        let command = resolve_agent_launch_command("codex", "powershell -NoProfile")
            .expect("resolve explicit custom command");

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let agents_json_exists = dir.join("agents.json").exists();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(command, format!("{} -NoProfile", custom.to_string_lossy()));
        assert!(!agents_json_exists);
    }

    #[test]
    fn resolved_executable_with_spaces_is_quoted_in_command() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let bin_dir = dir.join("space dir");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let scanned = write_command(&bin_dir, "codex");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", &bin_dir);

        let command = resolve_agent_launch_command("codex", "codex resume abc")
            .expect("resolve scanned command");

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            command,
            format!(
                "{} resume abc",
                quote_command_word(scanned.to_string_lossy().as_ref())
            )
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_launch_wrapper_does_not_write_agent_executable_config() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let bin_dir = dir.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        write_command(&bin_dir, "open");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", &bin_dir);

        let command = resolve_agent_launch_command("codex-desktop", "open -b com.openai.codex")
            .expect("resolve app launch command");

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let agents_json_exists = dir.join("agents.json").exists();
        let settings_json_exists = dir.join("settings.json").exists();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(command, "open -b com.openai.codex");
        assert!(!agents_json_exists);
        assert!(!settings_json_exists);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn shell_builtin_launch_does_not_write_agent_executable_config() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let previous_data_dir = std::env::var_os("VIBEAROUND_DATA_DIR");
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("VIBEAROUND_DATA_DIR", &dir);
        std::env::set_var("PATH", "");

        let command = resolve_agent_launch_command("codex-desktop", "Start-Process Codex")
            .expect("resolve app launch command");

        restore_env("VIBEAROUND_DATA_DIR", previous_data_dir);
        restore_env("PATH", previous_path);
        let agents_json_exists = dir.join("agents.json").exists();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(command, "Start-Process Codex");
        assert!(!agents_json_exists);
    }

    #[test]
    fn rejects_missing_command() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let fixture = PathFixture::empty();

        let error = validate_launch_command("codex").unwrap_err().to_string();

        drop(fixture);
        assert!(error.contains("was not found in PATH"));
    }

    #[test]
    fn resolves_executable_path() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let command = write_command(&dir, "agent");

        let path = resolve_executable_path(command.clone()).expect("resolve executable path");

        let _ = std::fs::remove_dir_all(&dir);
        assert!(path.is_absolute());
        assert!(path.ends_with("agent"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_executable_path() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("agent");
        std::fs::write(&path, "#!/bin/sh\n").expect("write command");

        let error = resolve_executable_path(path).unwrap_err().to_string();

        let _ = std::fs::remove_dir_all(&dir);
        assert!(error.contains("is not executable"));
    }

    struct PathFixture {
        dir: PathBuf,
        previous_path: Option<std::ffi::OsString>,
    }

    impl PathFixture {
        fn empty() -> Self {
            let dir = temp_dir();
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let previous_path = env::var_os("PATH");
            env::set_var("PATH", &dir);
            Self { dir, previous_path }
        }

        fn with_command(name: &str) -> Self {
            let fixture = Self::empty();
            fixture.write_command(name);
            fixture
        }

        fn write_command(&self, name: &str) -> PathBuf {
            write_command(&self.dir, name)
        }
    }

    impl Drop for PathFixture {
        fn drop(&mut self) {
            match &self.previous_path {
                Some(value) => env::set_var("PATH", value),
                None => env::remove_var("PATH"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "va-launch-executable-test-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn write_command(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").expect("write command");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("chmod command");
        }
        path
    }
}
