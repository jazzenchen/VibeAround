use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context;

const HEALTH_PROBE_ATTEMPTS: usize = 3;
const HEALTH_PROBE_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);
const HEALTH_PROBE_TOTAL_BUDGET: Duration = Duration::from_millis(750);
const HEALTH_PROBE_RETRY_DELAY: Duration = Duration::from_millis(50);

pub fn install_for_launch(agent_id: &str, workspace: &Path) -> anyhow::Result<()> {
    let integration_agent_id = project_integration_agent_id(agent_id);
    if server_is_running() {
        return common::agent::auto_install_project_integrations(integration_agent_id, workspace)
            .with_context(|| format!("install project integrations for {}", integration_agent_id));
    }

    eprintln!(
        "va-launch: daemon did not respond to the health probe; removing stale project integrations for {integration_agent_id}"
    );
    common::agent::uninstall_project_integrations(
        integration_agent_id,
        workspace,
        common::agent::ProjectIntegrationOptions {
            mcp: true,
            skills: true,
        },
    )
    .with_context(|| {
        format!(
            "remove stale project integrations for {}",
            integration_agent_id
        )
    })
}

fn project_integration_agent_id(agent_id: &str) -> &str {
    match agent_id {
        "claude-desktop" => "claude",
        "codex-desktop" => "codex",
        other => other,
    }
}

fn server_is_running() -> bool {
    let port = common::auth::read_token_file()
        .map(|auth| auth.port)
        .unwrap_or(common::config::DEFAULT_PORT);
    server_is_running_on_port(port)
}

fn server_is_running_on_port(port: u16) -> bool {
    let deadline = Instant::now() + HEALTH_PROBE_TOTAL_BUDGET;
    for attempt in 0..HEALTH_PROBE_ATTEMPTS {
        let Some(timeout) = remaining_probe_timeout(deadline) else {
            return false;
        };
        if probe_server_health_once(port, timeout) {
            return true;
        }
        if attempt + 1 < HEALTH_PROBE_ATTEMPTS {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            std::thread::sleep(std::cmp::min(HEALTH_PROBE_RETRY_DELAY, remaining));
        }
    }
    false
}

fn remaining_probe_timeout(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    Some(std::cmp::min(HEALTH_PROBE_ATTEMPT_TIMEOUT, remaining))
}

fn probe_server_health_once(port: u16, timeout: Duration) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let request =
        format!("GET /va/api/service/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }
    response.starts_with("HTTP/1.1 200")
        && (response.contains("\"service\":\"vibearound-server\"")
            || response.contains("\"service\": \"vibearound-server\""))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;

    use super::*;

    #[test]
    fn desktop_agents_install_companion_cli_integrations() {
        assert_eq!(project_integration_agent_id("codex-desktop"), "codex");
        assert_eq!(project_integration_agent_id("claude-desktop"), "claude");
        assert_eq!(project_integration_agent_id("gemini"), "gemini");
    }

    #[test]
    fn installs_project_scoped_integrations_for_companion_cli() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let workspace = dir.join("workspace");
        let data_dir = dir.join("data");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &data_dir);
        let (port, handle) = start_health_server();
        write_auth_file(&data_dir, port);
        common::config::reload();

        install_for_launch("codex-desktop", &workspace).expect("install integrations");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        common::config::reload();
        handle.join().expect("health server thread");
        let codex_config = workspace.join(".codex").join("config.toml");
        let codex_skill = workspace
            .join(".agents")
            .join("skills")
            .join("vibearound")
            .join("SKILL.md");

        assert!(codex_config.is_file());
        assert!(codex_skill.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_managed_project_integrations_when_server_is_not_running() {
        let _guard = crate::env_test_lock().lock().expect("env test lock");
        let dir = temp_dir();
        let workspace = dir.join("workspace");
        let data_dir = dir.join("data");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&data_dir).expect("create data dir");
        let previous = std::env::var_os("VIBEAROUND_DATA_DIR");
        std::env::set_var("VIBEAROUND_DATA_DIR", &data_dir);
        let port = unused_port();
        write_auth_file(&data_dir, port);
        common::config::reload();
        common::agent::install_project_integrations(
            "codex",
            &workspace,
            common::agent::ProjectIntegrationOptions {
                mcp: true,
                skills: true,
            },
        )
        .expect("seed managed integrations");

        install_for_launch("codex", &workspace).expect("remove stale integrations");

        restore_env("VIBEAROUND_DATA_DIR", previous);
        common::config::reload();
        assert!(!workspace
            .join(".agents")
            .join("skills")
            .join("vibearound")
            .exists());
        let config_body = std::fs::read_to_string(workspace.join(".codex").join("config.toml"))
            .unwrap_or_default();
        assert!(!config_body.contains("vibearound"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn health_probe_requires_vibearound_health_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake server");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}")
                .expect("write fake response");
        });

        assert!(!server_is_running_on_port(port));
        handle.join().expect("fake server thread");
    }

    #[test]
    fn health_probe_retries_transient_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind retry health server");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept first probe");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            drop(stream);

            let (mut stream, _) = listener.accept().expect("accept second probe");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 58\r\n\r\n{\"ok\":true,\"service\":\"vibearound-server\",\"version\":\"test\"}",
                )
                .expect("write health response");
        });

        assert!(server_is_running_on_port(port));
        handle.join().expect("retry health server thread");
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "va-launch-project-integration-test-{}",
            uuid::Uuid::new_v4()
        ))
    }

    fn start_health_server() -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind health server");
        let port = listener.local_addr().expect("local addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 58\r\n\r\n{\"ok\":true,\"service\":\"vibearound-server\",\"version\":\"test\"}",
                )
                .expect("write health response");
        });
        (port, handle)
    }

    fn unused_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused port");
        listener.local_addr().expect("local addr").port()
    }

    fn write_auth_file(data_dir: &Path, port: u16) {
        std::fs::write(
            data_dir.join("auth.json"),
            format!(r#"{{"port":{port},"token":"test-token"}}"#),
        )
        .expect("write auth file");
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
