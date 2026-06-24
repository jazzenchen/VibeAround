use va_client::http::AuthRequirement;
use va_client::ops;
use va_client::sessions::{CreateSessionBody, LaunchSessionInfo, PtyTool};

use super::{print_json, run_unit, transport_for};
use crate::args::{LaunchSessionMutationArgs, LaunchSessionsArgs, Options, SessionCreateArgs};
use crate::error::CliError;

pub(super) async fn list(options: &Options) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    if options.json {
        print_json(transport.execute_json(ops::sessions()).await?)?;
        return Ok(());
    }
    for session in transport.execute(ops::sessions()).await? {
        println!(
            "{}\t{:?}\t{}",
            session.session_id,
            session.status,
            session.project_path.unwrap_or_else(|| "-".into())
        );
    }
    Ok(())
}

pub(super) async fn launch_sessions(
    options: &Options,
    args: &LaunchSessionsArgs,
) -> Result<(), CliError> {
    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let agent_ids = if args.agent_ids.is_empty() {
        transport
            .execute(ops::runtime_agents())
            .await?
            .agents
            .into_iter()
            .map(|agent| agent.id)
            .collect::<Vec<_>>()
    } else {
        args.agent_ids.clone()
    };
    let agent_refs = agent_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let workspace_refs = args
        .workspace_paths
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let operation = ops::launch_sessions_batch(
        &agent_refs,
        &workspace_refs,
        Some(args.include_archived),
        args.limit,
    )?;

    if options.json {
        print_json(transport.execute_json(operation).await?)?;
        return Ok(());
    }

    for session in transport.execute(operation).await? {
        print_launch_session(session);
    }
    Ok(())
}

pub(super) async fn launch_session_mutation(
    options: &Options,
    args: &LaunchSessionMutationArgs,
    archived: bool,
) -> Result<(), CliError> {
    let operation = if archived {
        ops::launch_session_archive(
            &args.agent_id,
            &args.session_id,
            args.workspace_path.as_deref(),
        )?
    } else {
        ops::launch_session_unarchive(
            &args.agent_id,
            &args.session_id,
            args.workspace_path.as_deref(),
        )?
    };
    run_unit(
        options,
        operation,
        if archived {
            "launch session archived"
        } else {
            "launch session unarchived"
        },
    )
    .await
}

pub(super) async fn create(options: &Options, create: &SessionCreateArgs) -> Result<(), CliError> {
    if create.attach && options.json {
        return Err(CliError::Usage(
            "session create --attach does not support --json".into(),
        ));
    }

    let transport = transport_for(options, AuthRequirement::BearerToken)?;
    let operation = ops::session_create(CreateSessionBody {
        tool: create.tool,
        profile_id: create.profile_id.as_deref(),
        launch_target: create.launch_target.as_deref(),
        resume_session_id: create.resume_session_id.as_deref(),
        project_path: create.project_path.as_deref(),
        tmux_session: create.tmux_session.as_deref(),
        theme: create.theme.as_deref(),
        cols: create.cols,
        rows: create.rows,
    })?;
    if options.json {
        print_json(transport.execute_json(operation).await?)?;
        return Ok(());
    }

    let session = transport.execute(operation).await?;
    if create.attach {
        eprintln!("created session {}", session.session_id);
        return crate::attach::attach_session(options, &session.session_id).await;
    }

    println!("session: {}", session.session_id);
    println!("tool: {}", pty_tool_name(session.tool));
    println!("created_at: {}", session.created_at);
    if let Some(path) = session.project_path {
        println!("project: {path}");
    }
    if let Some(profile) = session.profile_label.or(session.profile_id) {
        println!("profile: {profile}");
    }
    if let Some(target) = session.launch_target {
        println!("target: {target}");
    }
    Ok(())
}

pub(super) async fn kill(options: &Options, session_id: &str) -> Result<(), CliError> {
    run_unit(options, ops::session_delete(session_id), "session killed").await
}

fn print_launch_session(session: LaunchSessionInfo) {
    println!("{}", launch_session_line(&session));
}

fn launch_session_line(session: &LaunchSessionInfo) -> String {
    let state = if session.active {
        "active"
    } else if session.archived {
        "archived"
    } else {
        "available"
    };

    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        session.agent_id,
        session.short_id,
        session.session_id,
        state,
        session.updated_at,
        session.workspace,
        session.title
    )
}

fn pty_tool_name(tool: PtyTool) -> &'static str {
    match tool {
        PtyTool::Generic => "generic",
        PtyTool::Claude => "claude",
        PtyTool::Codex => "codex",
        PtyTool::Pi => "pi",
        PtyTool::Gemini => "gemini",
        PtyTool::OpenCode => "opencode",
        PtyTool::Cursor => "cursor",
        PtyTool::Kiro => "kiro",
        PtyTool::QwenCode => "qwen-code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_session_line_includes_full_session_id() {
        let line = launch_session_line(&LaunchSessionInfo {
            agent_id: "codex".to_string(),
            session_id: "full-session-id".to_string(),
            title: "Fix bug".to_string(),
            workspace: "/tmp/project".to_string(),
            updated_at: 42,
            short_id: "abc123".to_string(),
            archived: false,
            active: false,
        });

        assert_eq!(
            line,
            "codex\tabc123\tfull-session-id\tavailable\t42\t/tmp/project\tFix bug"
        );
    }
}
