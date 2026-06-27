use std::env;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};

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
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some('"') if ch == '\\' => {
                if matches!(chars.peek(), Some('"') | Some('\\')) {
                    current.push(chars.next().expect("peeked next char"));
                } else {
                    current.push(ch);
                }
            }
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    return Some(current);
                }
            }
            None => current.push(ch),
        }
    }

    if current.is_empty() {
        None
    } else {
        Some(current)
    }
}

fn is_shell_builtin_for_current_platform(program: &str) -> bool {
    cfg!(target_os = "windows") && matches!(program, "Start-Process")
}

fn is_path_like_program(program: &str) -> bool {
    Path::new(program).is_absolute() || program.contains('/') || program.contains('\\')
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
