import { useState, type ReactNode } from "react";
import { Plus } from "lucide-react";
import { useI18n } from "@va/i18n";

import { getProfiles, type ProfileLaunchOption } from "@/api/sessions";
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
import { TOOL_OPTIONS, type ToolType } from "@/lib/terminal-types";
import { sessionToName } from "@/lib/session-mappers";

interface AddCliDropdownProps {
  /** "top" = inline next to tabs; "empty" = centered in empty state. */
  variant?: "top" | "empty";
  tmuxAvailable: boolean | null;
  tmuxSessions: string[];
  onAddCli: (tool: ToolType) => void;
  onAddProfileCli: (profileId: string, launchTarget: string) => void;
  onAttachTmux: (sessionName: string) => void;
  onRefreshTmux: () => void;
  trigger?: ReactNode;
}

/**
 * Shared "Add CLI" dropdown used in two places — the tab bar and the
 * empty state. Extracted to avoid duplicating ~55 lines of nearly
 * identical markup (the only differences were outer padding and input
 * font-size).
 */
export function AddCliDropdown({
  variant = "top",
  tmuxAvailable,
  tmuxSessions,
  onAddCli,
  onAddProfileCli,
  onAttachTmux,
  onRefreshTmux,
  trigger,
}: AddCliDropdownProps) {
  const { t } = useI18n();
  const [newTmuxName, setNewTmuxName] = useState("");
  const [profiles, setProfiles] = useState<ProfileLaunchOption[]>([]);
  const align = variant === "top" ? "start" : "center";
  const profileAgentGroups = groupProfilesByAgent(profiles);

  const refreshProfiles = () => {
    getProfiles()
      .then((items) => {
        setProfiles(items.filter((profile) => (profile.launch_targets ?? []).length > 0));
      })
      .catch((e) => console.error("[VibeAround] getProfiles:", e));
  };

  const submitNewTmux = () => {
    const name = newTmuxName.trim();
    if (!name) return;
    onAttachTmux(name);
    setNewTmuxName("");
  };

  const defaultTrigger = (
    <Button
      variant="ghost"
      size="sm"
      className={
        variant === "top"
          ? "h-auto gap-1 px-2.5 py-2 text-[11px] font-mono text-muted-foreground hover:text-foreground"
          : "gap-1.5 font-mono text-xs text-primary hover:bg-primary/10"
      }
      aria-label={t("Add CLI session")}
    >
      <Plus className="h-3.5 w-3.5" />
      {t("Add CLI")}
    </Button>
  );

  return (
    <DropdownMenu
      onOpenChange={(open) => {
        if (!open) return;
        onRefreshTmux();
        refreshProfiles();
      }}
    >
      <DropdownMenuTrigger asChild>{trigger ?? defaultTrigger}</DropdownMenuTrigger>
      <DropdownMenuContent
        className="max-h-[min(28rem,calc(100vh-1rem))] min-w-[180px] max-w-[min(24rem,calc(100vw-1rem))] overflow-y-auto p-1 font-mono text-xs"
        align={align}
      >
        <DropdownMenuLabel className="px-2 py-1.5 text-xs uppercase tracking-wider text-muted-foreground">
          {t("New session")}
        </DropdownMenuLabel>
        {TOOL_OPTIONS.map((tool) => (
          <DropdownMenuItem
            key={tool}
            onSelect={() => onAddCli(tool)}
            className="px-2 py-1.5 text-xs capitalize"
          >
            {sessionToName(tool)}
          </DropdownMenuItem>
        ))}
        {profileAgentGroups.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="px-2 py-1.5 text-xs uppercase tracking-wider text-muted-foreground">
              {t("Profiles")}
            </DropdownMenuLabel>
            <div className="hidden sm:block">
              {profileAgentGroups.map((group) => (
                <DropdownMenuSub key={group.agentId}>
                  <DropdownMenuSubTrigger className="px-2 py-1.5 text-xs">
                    {group.label}
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent className="min-w-[190px] max-w-[min(22rem,calc(100vw-1rem))] p-1 font-mono text-xs">
                    {group.items.map(({ profile, target }) => (
                      <DropdownMenuItem
                        key={`${profile.id}:${target.id}`}
                        onSelect={() => onAddProfileCli(profile.id, target.id)}
                        className="gap-2 px-2 py-1.5 text-xs"
                      >
                        <span className="min-w-0 flex-1 truncate">{profile.label}</span>
                        {target.bridge_target_api_type && (
                          <span className="shrink-0 text-muted-foreground/70">
                            {t("API bridge")}
                          </span>
                        )}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuSubContent>
                </DropdownMenuSub>
              ))}
            </div>
            <div className="space-y-1 sm:hidden">
              {profileAgentGroups.map((group) => (
                <div key={group.agentId}>
                  <div className="px-2 py-1 text-xs font-medium text-muted-foreground">
                    {group.label}
                  </div>
                  {group.items.map(({ profile, target }) => (
                    <DropdownMenuItem
                      key={`${profile.id}:${target.id}`}
                      onSelect={() => onAddProfileCli(profile.id, target.id)}
                      className="gap-2 px-4 py-1.5 text-xs"
                    >
                      <span className="min-w-0 flex-1 truncate">{profile.label}</span>
                      {target.bridge_target_api_type && (
                        <span className="shrink-0 text-muted-foreground/70">
                          {t("API bridge")}
                        </span>
                      )}
                    </DropdownMenuItem>
                  ))}
                </div>
              ))}
            </div>
          </>
        )}
        {tmuxAvailable && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="px-2 py-1.5 text-xs uppercase tracking-wider text-muted-foreground">
              {t("tmux sessions")}
            </DropdownMenuLabel>
            {tmuxSessions.map((name) => (
              <DropdownMenuItem
                key={`tmux-${name}`}
                onSelect={() => onAttachTmux(name)}
                className="px-2 py-1.5 text-xs"
              >
                ⎈ {name}
              </DropdownMenuItem>
            ))}
            <div
              className="flex items-center gap-1 px-2 py-1.5"
              onKeyDown={(e) => e.stopPropagation()}
            >
              <input
                type="text"
                placeholder={t("session name…")}
                value={newTmuxName}
                onChange={(e) => setNewTmuxName(e.target.value)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") submitNewTmux();
                }}
                className="min-w-0 flex-1 rounded border border-border/40 bg-transparent px-1.5 py-0.5 font-mono text-xs text-foreground outline-none placeholder:text-muted-foreground/40 focus:border-primary/50"
              />
              <Button
                variant="ghost"
                size="icon-xs"
                className="shrink-0 h-5 w-5 text-muted-foreground/60 hover:text-primary"
                disabled={!newTmuxName.trim()}
                onClick={submitNewTmux}
              >
                <Plus className="h-3 w-3" />
              </Button>
            </div>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

type ProfileLaunchTarget = ProfileLaunchOption["launch_targets"][number];

interface ProfileAgentGroup {
  agentId: string;
  label: string;
  items: Array<{
    profile: ProfileLaunchOption;
    target: ProfileLaunchTarget;
  }>;
}

function groupProfilesByAgent(profiles: ProfileLaunchOption[]): ProfileAgentGroup[] {
  const groups: ProfileAgentGroup[] = [];
  const byAgent = new Map<string, ProfileAgentGroup>();

  for (const profile of profiles) {
    for (const target of profile.launch_targets ?? []) {
      let group = byAgent.get(target.id);
      if (!group) {
        group = { agentId: target.id, label: target.label, items: [] };
        byAgent.set(target.id, group);
        groups.push(group);
      }
      group.items.push({ profile, target });
    }
  }

  return groups;
}
