import type { AgentSummary, LauncherPreferences } from "../Launch/api";
import { agentProfileId } from "../Launch/launchModel";
import type { Settings as AppSettings } from "../Onboarding/types";
import {
  DIRECT_PROFILE,
  FOLLOW_DEFAULT,
  type AppDefaultForm,
  type ChannelDefaultForm,
  type RemoteChannelDefaults,
  type RemoteSettings,
} from "./types";

const INTERNAL_CHANNEL_IDS = new Set(["web"]);

export function parseRemoteSettings(settings: AppSettings): RemoteSettings {
  const remote = isRecord(settings.remote) ? settings.remote : {};
  const channels = isRecord(remote.channels) ? remote.channels : {};
  const parsedChannels: Record<string, RemoteChannelDefaults> = {};
  for (const [id, value] of Object.entries(channels)) {
    if (isRecord(value)) parsedChannels[id] = value as RemoteChannelDefaults;
  }
  return { channels: parsedChannels };
}

export function configuredChannelIdsFromSettings(
  settings: AppSettings,
  remote: RemoteSettings,
): string[] {
  const ids = new Set<string>();
  if (isRecord(settings.channels)) {
    Object.keys(settings.channels)
      .filter(isUserFacingChannel)
      .forEach((id) => ids.add(id));
  }
  Object.keys(remote.channels ?? {})
    .filter(isUserFacingChannel)
    .forEach((id) => ids.add(id));
  return [...ids];
}

export function readImChannelOrder(settings: AppSettings): string[] {
  const im = isRecord(settings.im) ? settings.im : {};
  return uniqueStrings(Array.isArray(im.order) ? im.order : []);
}

export function orderChannelIds(channelIds: string[], order: string[]): string[] {
  const rank = new Map(uniqueStrings(order).map((id, index) => [id, index]));
  return [...channelIds].sort((left, right) => {
    const leftRank = rank.get(left);
    const rightRank = rank.get(right);
    if (leftRank !== undefined && rightRank !== undefined) {
      return leftRank - rightRank;
    }
    if (leftRank !== undefined) return -1;
    if (rightRank !== undefined) return 1;
    return channelDisplayName(left).localeCompare(channelDisplayName(right));
  });
}

export function moveChannelOrder(
  channelIds: string[],
  fromId: string,
  toId: string,
): string[] {
  if (fromId === toId) return channelIds;
  const fromIndex = channelIds.indexOf(fromId);
  const toIndex = channelIds.indexOf(toId);
  if (fromIndex < 0 || toIndex < 0) return channelIds;
  const next = [...channelIds];
  const [item] = next.splice(fromIndex, 1);
  next.splice(toIndex, 0, item);
  return next;
}

export function writeImChannelOrder(
  settings: AppSettings,
  order: string[],
): AppSettings {
  const result: AppSettings = { ...settings };
  const im = isRecord(settings.im) ? { ...settings.im } : {};
  const normalized = uniqueStrings(order);
  if (normalized.length > 0) {
    im.order = normalized;
  } else {
    delete im.order;
  }

  if (Object.keys(im).length > 0) {
    result.im = im;
  } else {
    delete result.im;
  }
  return result;
}

function isUserFacingChannel(id: string): boolean {
  return !INTERNAL_CHANNEL_IDS.has(id);
}

export function formForChannel(
  remote: RemoteSettings,
  channelId: string,
): ChannelDefaultForm {
  const entry = remote.channels?.[channelId] ?? {};
  return {
    agentId: stringValue(entry.agent_id) ?? FOLLOW_DEFAULT,
    profileId: stringValue(entry.profile_id) ?? FOLLOW_DEFAULT,
  };
}

export function formForAppDefault(
  prefs: LauncherPreferences | null,
  agents: AgentSummary[],
): AppDefaultForm {
  const agentId = prefs?.defaultAgent ?? agents[0]?.id ?? "codex";
  return {
    agentId,
    profileId: prefs?.defaultProfileId ?? DIRECT_PROFILE,
  };
}

export function defaultChannelForm(): ChannelDefaultForm {
  return {
    agentId: FOLLOW_DEFAULT,
    profileId: FOLLOW_DEFAULT,
  };
}

export function defaultAppDefaultForm(): AppDefaultForm {
  return {
    agentId: "codex",
    profileId: DIRECT_PROFILE,
  };
}

export function updateRemoteChannelForm(
  settings: AppSettings,
  channelId: string,
  form: ChannelDefaultForm,
): AppSettings {
  const result: AppSettings = { ...settings };
  const remote = isRecord(settings.remote) ? { ...settings.remote } : {};
  const existingChannels = isRecord(remote.channels) ? remote.channels : {};
  const channels: Record<string, RemoteChannelDefaults> = {};
  for (const [id, value] of Object.entries(existingChannels)) {
    if (isRecord(value)) channels[id] = { ...(value as RemoteChannelDefaults) };
  }

  const entry: RemoteChannelDefaults = { ...(channels[channelId] ?? {}) };
  if (form.agentId === FOLLOW_DEFAULT) delete entry.agent_id;
  else entry.agent_id = form.agentId;
  if (form.profileId === FOLLOW_DEFAULT) delete entry.profile_id;
  else entry.profile_id = form.profileId;

  if (Object.keys(entry).length > 0) channels[channelId] = entry;
  else delete channels[channelId];

  if (Object.keys(channels).length > 0) {
    remote.channels = channels;
  } else {
    delete remote.channels;
  }

  if (Object.keys(remote).length > 0) {
    result.remote = remote;
  } else {
    delete result.remote;
  }
  return result;
}

export function resolvedProfileIdForChannel(
  form: ChannelDefaultForm,
  prefs: LauncherPreferences | null,
  agentId: string,
): string | undefined {
  return form.profileId === FOLLOW_DEFAULT
    ? prefs
      ? agentProfileId(prefs, agentId)
      : undefined
    : form.profileId;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function uniqueStrings(values: unknown[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    if (typeof value !== "string") continue;
    const trimmed = value.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    result.push(trimmed);
  }
  return result;
}

function channelDisplayName(kind: string) {
  const known: Record<string, string> = {
    dingtalk: "DingTalk",
    discord: "Discord",
    feishu: "Feishu",
    qqbot: "QQ Bot",
    slack: "Slack",
    telegram: "Telegram",
    wechat: "WeChat",
    wecom: "WeCom",
  };
  return known[kind] ?? capitalize(kind);
}

function capitalize(value: string): string {
  return value.length === 0 ? value : value[0].toUpperCase() + value.slice(1);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
