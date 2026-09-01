use std::path::PathBuf;

mod chat;
mod cli;
mod launch;
mod pair;
mod serve;

#[cfg(test)]
mod tests;

pub(crate) use chat::{ChatForgetArgs, ChatReplArgs, ChatSendArgs};
pub(crate) use cli::parse_args;
pub(crate) use launch::{LaunchRunArgs, LaunchSessionMutationArgs, LaunchSessionsArgs};
pub(crate) use pair::{PairStartArgs, PairWaitArgs};
pub(crate) use serve::ServeArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help,
    Health,
    Info,
    Status,
    Doctor,
    Channels,
    Tunnels,
    Agents,
    Workspaces,
    Previews,
    Profiles,
    Serve(ServeArgs),
    ChatSend(ChatSendArgs),
    ChatRepl(ChatReplArgs),
    ChatSessions,
    ChatForget(ChatForgetArgs),
    PairStart(PairStartArgs),
    PairStatus { sid: String, save: bool },
    PairWait(PairWaitArgs),
    AuthStatus,
    AuthClear,
    SettingsReload,
    LaunchRun(LaunchRunArgs),
    LaunchSessions(LaunchSessionsArgs),
    LaunchSessionArchive(LaunchSessionMutationArgs),
    LaunchSessionUnarchive(LaunchSessionMutationArgs),
    ChannelSync,
    ChannelStart { kind: String },
    ChannelStop { kind: String },
    ChannelRestart { kind: String },
    TunnelKill { provider: String },
    AgentKill { thread_id: String },
    PreviewDelete { slug: String },
    WorkspaceAdd { path: String },
    WorkspaceRemove { path: String },
    WorkspaceDefault { path: String },
    WorkspaceCreate { name: String },
}

#[derive(Debug, Default)]
pub(crate) struct Options {
    pub(crate) command: Option<Command>,
    pub(crate) auth_file: Option<PathBuf>,
    pub(crate) base_url: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) json: bool,
}

pub(super) fn parse_positive_u64(value: &str) -> Result<u64, String> {
    let value = value
        .parse::<u64>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if value == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(value)
}

pub(crate) fn usage() -> &'static str {
    concat!(
        "Usage: va [--auth-file PATH] [--base-url URL] [--token TOKEN] [--json] <command>\n\n",
        "Commands:\n",
        "  help                         Show this help\n",
        "  health                       Check public server liveness\n",
        "  info                         Show server metadata\n",
        "  status                       Show a compact runtime summary\n",
        "  doctor                       Diagnose endpoint, auth, and server health\n",
        "  serve                        Start the standalone VibeAround server\n",
        "  auth status                  Show resolved auth configuration\n",
        "  auth clear                   Remove the saved auth file\n",
        "  pair start                   Start browser/IM pairing\n",
        "  pair start --wait --save     Start pairing, wait for verification, then save auth\n",
        "  pair status SID [--save]     Poll pairing; save verified local auth with --save\n",
        "  pair wait SID [--save]       Wait for pairing verification\n",
        "  chat send TEXT               Send one prompt over /ws/chat and wait for completion\n",
        "  chat send --stdin            Read one prompt from standard input\n",
        "  chat repl                    Start a line-oriented chat session\n",
        "  chat send --continue TEXT    Resume the saved chat session for this workspace\n",
        "  chat sessions                List locally saved chat sessions\n",
        "  chat forget [--all]          Forget a saved chat session scope\n",
        "  channels                     List channel plugin runtimes\n",
        "  channel sync                 Reconcile channel plugins with settings\n",
        "  channel start KIND           Start a stopped channel plugin\n",
        "  channel stop KIND            Stop a channel plugin\n",
        "  channel restart KIND         Restart a channel plugin\n",
        "  tunnels                      List tunnel runtimes\n",
        "  tunnel kill PROVIDER         Stop a tunnel runtime\n",
        "  agents                       List enabled agents\n",
        "  agent kill THREAD_ID         Kill an agent runtime\n",
        "  launch --profile NAME        Launch a saved va-launch profile\n",
        "  launch --profile-path PATH    Launch a va-launch profile JSON file\n",
        "  launch sessions              List resumable agent launch sessions\n",
        "  launch archive --agent A ID  Archive a launch session\n",
        "  launch unarchive --agent A ID Unarchive a launch session\n",
        "  workspaces                   List registered workspaces\n",
        "  workspace add PATH           Register a workspace path\n",
        "  workspace remove PATH        Remove a workspace path\n",
        "  workspace default PATH       Set the default workspace\n",
        "  workspace create NAME        Create a workspace under the default root\n",
        "  previews                     List live previews\n",
        "  preview delete SLUG          Close a live preview\n",
        "  profiles                     List model profiles\n",
        "  settings reload              Reload server settings",
    )
}
