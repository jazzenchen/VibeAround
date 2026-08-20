"use client";

import type { AgentInfo, ProfileLaunchOption } from "@va/client";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { cn } from "@/lib/utils";
import {
  DIRECT_PROFILE_ID,
  launchProfilesForAgent,
  profileIdForAgent,
} from "./chatLaunchContract";

interface NewChatAgentPickerProps {
  agents: AgentInfo[];
  profiles: ProfileLaunchOption[];
  selectedAgentId: string;
  selectedProfileId?: string;
  fallbackAgentLabel: string;
  onLaunchChange: (agentId: string, profileId?: string) => void;
  className?: string;
}

function SectionTitle({ label }: { label: string }) {
  return (
    <div className="mb-2 px-1 font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/60">
      {label}
    </div>
  );
}

export function NewChatAgentPicker({
  agents,
  profiles,
  selectedAgentId,
  selectedProfileId,
  fallbackAgentLabel,
  onLaunchChange,
  className,
}: NewChatAgentPickerProps) {
  const { t } = useI18n();
  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId);
  const selectedAgentName = selectedAgent?.name ?? fallbackAgentLabel;
  const selectedAgentProfiles = launchProfilesForAgent(profiles, selectedAgentId);

  const chooseAgent = (agent: AgentInfo) => {
    const nextProfileId = profileIdForAgent(agent, profiles, selectedProfileId);
    onLaunchChange(agent.id, nextProfileId);
  };

  const optionButtonClass = (selected: boolean) =>
    cn(
      "inline-flex max-w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs transition-colors",
      selected
        ? "bg-primary/10 text-foreground"
        : "text-muted-foreground hover:bg-muted/70 hover:text-foreground",
    );

  return (
    <>
      <section className={cn("w-full", className)}>
        <SectionTitle label={t("Agent")} />
        <div className="flex flex-wrap items-center gap-1.5">
          {agents.length === 0 ? (
            <div
              className="inline-flex max-w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-xs text-muted-foreground"
              title={selectedAgentName}
              aria-label={selectedAgentName}
            >
              <BrandIcon
                kind="cli"
                id={selectedAgentId}
                label={selectedAgentName}
                className="h-5 w-5"
              />
              <span className="min-w-0 flex-1 truncate font-medium">{selectedAgentName}</span>
            </div>
          ) : (
            agents.map((agent) => {
              const selected = agent.id === selectedAgentId;
              return (
                <button
                  key={agent.id}
                  type="button"
                  className={optionButtonClass(selected)}
                  aria-pressed={selected}
                  aria-label={agent.name}
                  title={agent.name}
                  onClick={() => chooseAgent(agent)}
                >
                  <BrandIcon kind="cli" id={agent.id} label={agent.name} className="h-5 w-5" />
                  <span className="min-w-0 flex-1 truncate font-medium">{agent.name}</span>
                </button>
              );
            })
          )}
        </div>
      </section>

      <section className={cn("w-full", className)}>
        <SectionTitle label={t("Profile")} />
        <div className="flex flex-wrap items-center gap-1.5">
          {!selectedAgent?.requires_profile && (
            <button
              type="button"
              className={optionButtonClass(selectedProfileId === DIRECT_PROFILE_ID)}
              aria-pressed={selectedProfileId === DIRECT_PROFILE_ID}
              onClick={() => onLaunchChange(selectedAgentId, DIRECT_PROFILE_ID)}
            >
              <BrandIcon
                kind="cli"
                id={selectedAgentId}
                label={selectedAgentName}
                className="h-5 w-5"
              />
              <span className="min-w-0 flex-1 truncate font-medium">{t("Direct")}</span>
            </button>
          )}
          {selectedAgentProfiles.map((profile) => (
            <button
              key={profile.id}
              type="button"
              className={optionButtonClass(selectedProfileId === profile.id)}
              aria-pressed={selectedProfileId === profile.id}
              title={profile.label}
              onClick={() => onLaunchChange(selectedAgentId, profile.id)}
            >
              <BrandIcon
                kind="provider"
                id={profile.provider}
                label={profile.label}
                fallback={profile.label.slice(0, 1).toUpperCase()}
                className="h-5 w-5"
              />
              <span className="min-w-0 flex-1 truncate font-medium">{profile.label}</span>
            </button>
          ))}
        </div>
      </section>
    </>
  );
}
