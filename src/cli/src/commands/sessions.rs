use va_client::http::AuthRequirement;
use va_client::ops;
use va_client::sessions::LaunchSessionInfo;

use super::{print_json, run_unit, transport_for};
use crate::args::{LaunchSessionMutationArgs, LaunchSessionsArgs, Options};
use crate::error::CliError;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_session_line_includes_full_session_id() {
        let line = launch_session_line(&LaunchSessionInfo {
            agent_id: "codex".to_string(),
            host_agent_id: None,
            host_profile_id: None,
            host_profile_label: None,
            host_provider: None,
            host_provider_label: None,
            session_id: "full-session-id".to_string(),
            title: "Fix bug".to_string(),
            workspace: "/tmp/project".to_string(),
            updated_at: 42,
            short_id: "abc123".to_string(),
            archived: false,
            active: false,
            thread_id: None,
        });

        assert_eq!(
            line,
            "codex\tabc123\tfull-session-id\tavailable\t42\t/tmp/project\tFix bug"
        );
    }
}
