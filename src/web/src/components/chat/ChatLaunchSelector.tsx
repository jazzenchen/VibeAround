"use client";

import { ChevronDown } from "lucide-react";
import type { AgentInfo, ProfileLaunchOption } from "@va/client";
import { useI18n } from "@va/i18n";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ToolType } from "@/lib/terminal-types";
import { getToolTheme } from "@/lib/terminal-types";
import { useTheme } from "@/lib/theme";
import {
  COMPACT_MENU_ITEM,
  COMPACT_MENU_LABEL,
  COMPACT_SEPARATOR,
  COMPACT_SUB_TRIGGER,
} from "./chatPickerStyles";

const DIRECT_PROFILE_ID = "direct";

export interface ChatLaunchSelectorProps {
  targetLabel: string;
  targetTool: ToolType;
  selectedAgentId?: string;
  agents: AgentInfo[];
  profiles?: ProfileLaunchOption[];
  selectedProfileId?: string;
  onAgentChange?: (agentId: string) => void;
  onLaunchChange?: (agentId: string, profileId?: string) => void;
}

export function ChatLaunchSelector({
  targetLabel,
  targetTool,
  selectedAgentId,
  agents,
  profiles = [],
  selectedProfileId,
  onAgentChange,
  onLaunchChange,
}: ChatLaunchSelectorProps) {
  const { t } = useI18n();
  const appTheme = useTheme();
  const accentColor = getToolTheme(targetTool, appTheme).accent;
  const currentAgentId = selectedAgentId ?? targetTool;
  const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId);
  const activeProfileId = selectedProfileId ?? DIRECT_PROFILE_ID;
  const selectedRouteLabel =
    activeProfileId === DIRECT_PROFILE_ID
      ? t("{{agent}} / Direct", { agent: targetLabel })
      : selectedProfile
        ? t("{{agent}} / {{profile}}", { agent: targetLabel, profile: selectedProfile.label })
        : targetLabel;
  const hasMenu = agents.length > 0 && (onLaunchChange || onAgentChange);
  const directAgents = agents.filter((agent) => !agent.requires_profile);

  const launchProfilesForAgent = (agentId: string) =>
    profiles.flatMap((profile) => {
      const target = profile.launch_targets.find((target) => target.id === agentId);
      return target ? [{ profile, usesBridge: Boolean(target.bridge_target_api_type) }] : [];
    });

  const chooseLaunch = (agentId: string, profileId?: string) => {
    if (onLaunchChange) {
      onLaunchChange(agentId, profileId);
    } else {
      onAgentChange?.(agentId);
    }
  };

  if (!hasMenu) {
    return (
      <span
        className="flex min-w-0 items-center gap-1 truncate text-xs font-medium"
        title={targetLabel}
      >
        <span className="truncate" style={{ color: accentColor }}>
          {targetLabel}
        </span>
      </span>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="h-6 min-w-0 max-w-[18rem] justify-start gap-1 px-1 text-xs font-medium"
          title={selectedRouteLabel}
        >
          <span className="truncate" style={{ color: accentColor }}>
            {selectedRouteLabel}
          </span>
          <ChevronDown className="h-3 w-3 shrink-0 text-muted-foreground" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="top"
        align="start"
        className="max-h-[18rem] min-w-[210px] max-w-[min(22rem,calc(100vw-1rem))] overflow-y-auto p-0.5 text-xs"
      >
        {directAgents.length > 0 && (
          <>
            <DropdownMenuSub>
              <DropdownMenuSubTrigger className={COMPACT_SUB_TRIGGER}>
                {t("Launch without profile")}
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-[190px] p-0.5 text-xs">
                {directAgents.map((agent) => (
                  <DropdownMenuItem
                    key={agent.id}
                    onClick={() => chooseLaunch(agent.id, DIRECT_PROFILE_ID)}
                    className={`flex items-center justify-between ${COMPACT_MENU_ITEM}`}
                  >
                    <span className="truncate">{agent.name}</span>
                    {currentAgentId === agent.id && activeProfileId === DIRECT_PROFILE_ID && (
                      <span className="text-[11px] text-muted-foreground">{t("current")}</span>
                    )}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
            <DropdownMenuSeparator className={COMPACT_SEPARATOR} />
          </>
        )}
        <DropdownMenuLabel className={COMPACT_MENU_LABEL}>{t("Profiles")}</DropdownMenuLabel>
        {agents.map((agent) => {
          const entries = launchProfilesForAgent(agent.id);
          if (!entries.length) return null;
          return (
            <DropdownMenuSub key={agent.id}>
              <DropdownMenuSubTrigger className={COMPACT_SUB_TRIGGER}>
                {agent.name}
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent className="min-w-[220px] max-w-[22rem] p-0.5 text-xs">
                {entries.map(({ profile, usesBridge }) => (
                  <DropdownMenuItem
                    key={profile.id}
                    onClick={() => chooseLaunch(agent.id, profile.id)}
                    className={`flex items-center justify-between gap-2 ${COMPACT_MENU_ITEM}`}
                  >
                    <span className="truncate">
                      {usesBridge
                        ? t("{{profile}} (API bridge)", { profile: profile.label })
                        : profile.label}
                    </span>
                    {currentAgentId === agent.id && activeProfileId === profile.id && (
                      <span className="text-[11px] text-muted-foreground">{t("current")}</span>
                    )}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          );
        })}
        {!profiles.length &&
          directAgents.map((agent) => (
            <DropdownMenuItem
              key={agent.id}
              onClick={() => chooseLaunch(agent.id, DIRECT_PROFILE_ID)}
              className={`flex items-center justify-between ${COMPACT_MENU_ITEM}`}
            >
              <span className="truncate">{agent.name}</span>
              {agent.id === currentAgentId && (
                <span className="text-[11px] text-muted-foreground">{t("current")}</span>
              )}
            </DropdownMenuItem>
          ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
