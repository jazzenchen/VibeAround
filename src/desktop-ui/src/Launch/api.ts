import { invoke } from "@tauri-apps/api/core";

import type {
  CatalogEntry,
  CompatibilityBridgeMode,
  AgentLaunchArgs,
  AgentLaunchPreference,
  ConnectionAgentId,
  ProfileDef,
  ProfileDraft,
  ProfileConnectionPreference,
  ProfileConnections,
  ProfileSummary,
} from "./types";
import type { Settings } from "../Onboarding/types";

export function listProfiles(): Promise<ProfileSummary[]> {
  return invoke<ProfileSummary[]>("profiles_list");
}

export function getProfile(id: string): Promise<ProfileDef> {
  return invoke<ProfileDef>("profiles_get", { id });
}

export function upsertProfile(id: string, draft: ProfileDraft): Promise<void> {
  return invoke<void>("profiles_upsert", { id, draft });
}

export function createProfile(draft: ProfileDraft): Promise<ProfileDef> {
  return invoke<ProfileDef>("profiles_create", { draft });
}

export function deleteProfile(id: string): Promise<void> {
  return invoke<void>("profiles_delete", { id });
}

export function reorderProfiles(profileIds: string[]): Promise<void> {
  return invoke<void>("profiles_reorder", { profileIds });
}

export function launchProfile(id: string, launchTarget: string): Promise<void> {
  return invoke<void>("profiles_launch", { id, launchTarget });
}

export function launchProfileResume(
  id: string,
  launchTarget: string,
  sessionId: string,
): Promise<void> {
  return invoke<void>("profiles_launch_resume", {
    id,
    launchTarget,
    sessionId,
  });
}

export function launchDefault(): Promise<void> {
  return invoke<void>("profiles_launch_default");
}

/** Direct launch — no env, CLI uses whatever global OAuth the user has. */
export function launchDirect(agentId: string): Promise<void> {
  return invoke<void>("profiles_launch_direct", { agentId });
}

export function launchDirectResume(
  agentId: string,
  sessionId: string,
): Promise<void> {
  return invoke<void>("profiles_launch_direct_resume", { agentId, sessionId });
}

export interface GoogleOAuthStatus {
  signedIn: boolean;
  path: string;
  expiresAt?: number | null;
}

export function googleOAuthStatus(): Promise<GoogleOAuthStatus> {
  return invoke<GoogleOAuthStatus>("profiles_google_oauth_status");
}

export function googleOAuthLogin(): Promise<GoogleOAuthStatus> {
  return invoke<GoogleOAuthStatus>("profiles_google_oauth_login");
}

export interface ProfileConnectionTestResult {
  testedApiTypes: string[];
}

export function testProfileConnection(
  draft: ProfileDraft,
): Promise<ProfileConnectionTestResult> {
  return invoke<ProfileConnectionTestResult>("profiles_test_connection", {
    draft,
  });
}

export interface AgentSummary {
  id: string;
  display_name: string;
  description: string;
  install_type: string | null;
  pty_command: string;
  direct_only: boolean;
  acp_program: string;
  acp_args: string[];
  acp_npm_package?: string | null;
  acp_bin_name?: string | null;
}

/** Reuses the onboarding command that returns all registered CLIs. */
export function listAgents(): Promise<AgentSummary[]> {
  return invoke<AgentSummary[]>("list_agents");
}

export function rescanAgentEntries(): Promise<unknown> {
  return invoke("rescan_agent_entries");
}

export interface AgentExecutableCandidate {
  path: string;
  realpath?: string | null;
  version?: string | null;
  latestVersion?: string | null;
  updateAvailable?: boolean | null;
  source: string;
  sourceLabel: string;
  rank: number;
  selected: boolean;
  updateCommand?: string | null;
}

export interface AgentExecutableResolution {
  agentId: string;
  configuredPath?: string | null;
  selected?: AgentExecutableCandidate | null;
  candidates: AgentExecutableCandidate[];
}

export interface AgentExecutableLatest {
  path: string;
  latestVersion?: string | null;
  updateAvailable?: boolean | null;
}

export function getAgentExecutableResolution(
  agentId: string,
  options?: { scan?: boolean },
): Promise<AgentExecutableResolution> {
  return invoke<AgentExecutableResolution>(
    "launcher_agent_executable_resolution",
    {
      agentId,
      scan: options?.scan ?? false,
    },
  );
}

export function getAgentExecutableLatest(
  agentId: string,
  executablePath: string,
): Promise<AgentExecutableLatest> {
  return invoke<AgentExecutableLatest>("launcher_agent_executable_latest", {
    agentId,
    executablePath,
  });
}

export function updateLauncherAgent(
  agentId: string,
  executablePath?: string | null,
): Promise<void> {
  return invoke<void>("launcher_update_agent", {
    agentId,
    executablePath: executablePath ?? null,
  });
}

export interface DesktopAppDetectionFile {
  apps: Record<string, DesktopAppDetection>;
}

export interface DesktopAppDetection {
  installed: boolean;
  launchCommand: string;
  entry?: {
    appName: string;
    path: string;
    source: string;
    sourceLabel: string;
  } | null;
}

export function rescanDesktopAppEntries(): Promise<DesktopAppDetectionFile> {
  return invoke<DesktopAppDetectionFile>("rescan_desktop_app_entries");
}

export function getDesktopAppEntries(): Promise<DesktopAppDetectionFile | null> {
  return invoke<DesktopAppDetectionFile | null>("get_desktop_app_entries");
}

export interface WindowsStartAppEntry {
  name: string;
  appId: string;
  recommended: boolean;
}

export function listWindowsStartApps(
  agentId: string,
): Promise<WindowsStartAppEntry[]> {
  return invoke<WindowsStartAppEntry[]>("list_windows_start_apps", {
    agentId,
  });
}

export async function getDesktopAppEntriesForAgents(
  agents: Pick<AgentSummary, "id" | "direct_only">[],
): Promise<DesktopAppDetectionFile | null> {
  const cached = await getDesktopAppEntries().catch(() => null);
  const needsScan =
    !cached ||
    agents.some((agent) => agent.direct_only && !(agent.id in cached.apps));
  if (!needsScan) return cached;
  return rescanDesktopAppEntries().catch(() => cached);
}

export function checkSelectedLaunchEntry(): Promise<boolean> {
  return invoke<boolean>("check_selected_launch_entry");
}

export interface TerminalOption {
  id: string;
  label: string;
  installed: boolean;
}

export interface LauncherPreferences {
  terminal: string;
  options: TerminalOption[];
  workspace: string;
  workspaceOptions: WorkspaceOption[];
  selectedAgent: string;
  agentPreferences: Record<string, AgentLaunchPreference>;
  defaultAgent: string;
  defaultProfileId?: string | null;
  enabledAgents: string[];
  defaultProfiles: Record<string, string>;
  compatibilityBridge: CompatibilityBridgeMode;
  localAgentApiEnabled: boolean;
  profileConnections: ProfileConnections;
}

export interface LaunchSessionSummary {
  agentId: string;
  sessionId: string;
  title: string;
  workspace: string;
  updatedAt: number;
  shortId: string;
  archived: boolean;
}

export interface WorkspaceOption {
  path: string;
  label: string;
  detail: string;
  kind: string;
  isDefault: boolean;
}

export function getLauncherPreferences(): Promise<LauncherPreferences> {
  return invoke<LauncherPreferences>("launcher_get_preferences");
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function listLauncherWorkspaces(
  agentId?: string,
): Promise<WorkspaceOption[]> {
  return invoke<WorkspaceOption[]>("launcher_list_workspaces", {
    agentId: agentId ?? null,
  });
}

export function listLaunchSessions(
  agentId: string,
  workspacePath: string,
  includeArchived = false,
): Promise<LaunchSessionSummary[]> {
  return invoke<LaunchSessionSummary[]>("launcher_list_sessions", {
    agentId,
    workspacePath,
    includeArchived,
  });
}

export function setLauncherTerminal(terminalId: string): Promise<void> {
  return invoke<void>("launcher_set_terminal", { terminalId });
}

export function setLauncherWorkspace(
  workspacePath: string,
  agentId?: string,
): Promise<void> {
  return invoke<void>("launcher_set_workspace", {
    workspacePath,
    agentId: agentId ?? null,
  });
}

export function removeLauncherWorkspace(workspacePath: string): Promise<void> {
  return invoke<void>("launcher_remove_workspace", { workspacePath });
}

export function reorderLauncherWorkspaces(
  workspacePaths: string[],
): Promise<void> {
  return invoke<void>("launcher_reorder_workspaces", { workspacePaths });
}

export function setLauncherCompatibilityBridge(
  mode: CompatibilityBridgeMode,
): Promise<void> {
  return invoke<void>("launcher_set_compatibility_bridge", { mode });
}

export function setLauncherLocalAgentApiEnabled(
  enabled: boolean,
): Promise<void> {
  return invoke<void>("launcher_set_local_agent_api_enabled", { enabled });
}

export function setProfileConnection(
  profileId: string,
  agentId: ConnectionAgentId,
  preference: ProfileConnectionPreference,
): Promise<void> {
  return invoke<void>("launcher_set_profile_connection", {
    profileId,
    agentId,
    preference,
  });
}

export function setLauncherDefault(
  agentId: string,
  profileId: string | null,
): Promise<void> {
  return invoke<void>("launcher_set_default", { agentId, profileId });
}

export function setLauncherAgentProfile(
  agentId: string,
  profileId: string | null,
): Promise<void> {
  return invoke<void>("launcher_set_agent_profile", { agentId, profileId });
}

export function setLauncherAgentLaunchArgs(
  agentId: string,
  launchArgs: AgentLaunchArgs,
): Promise<void> {
  return invoke<void>("launcher_set_agent_launch_args", {
    agentId,
    launchArgs,
  });
}

export function setLauncherAgentExecutablePath(
  agentId: string,
  executablePath: string | null,
  executableSourceLabel?: string | null,
): Promise<void> {
  return invoke<void>("launcher_set_agent_executable_path", {
    agentId,
    executablePath,
    executableSourceLabel: executableSourceLabel ?? null,
  });
}

export function setLauncherSelectedAgent(agentId: string): Promise<void> {
  return invoke<void>("launcher_set_selected_agent", { agentId });
}

export function listCatalog(): Promise<CatalogEntry[]> {
  return invoke<CatalogEntry[]>("profiles_catalog");
}
