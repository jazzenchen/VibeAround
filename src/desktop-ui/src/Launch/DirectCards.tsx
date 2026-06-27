import { useEffect, useMemo, useState } from "react";
import { Pin, Sparkles, Star } from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import {
  getDesktopAppEntriesForAgents,
  listAgents,
  type AgentSummary,
} from "./api";

const AGENT_DISPLAY_ORDER = [
  "claude",
  "claude-desktop",
  "codex",
  "codex-desktop",
  "pi",
  "gemini",
  "opencode",
  "cursor",
  "kiro",
  "qwen-code",
];

interface Props {
  onLaunch: (agentId: string) => Promise<void>;
  onSetDefault: (agentId: string) => Promise<void>;
  busy: boolean;
  defaultAgent?: string;
  enabledAgents?: string[];
  defaultProfiles?: Record<string, string>;
}

/**
 * "直接启动" — fire any of the registered coder CLIs
 * with no env at all. The CLI does its own thing (OAuth, cached token,
 * provider config, …); VibeAround does not touch credentials.
 *
 * Buttons are populated dynamically from the daemon's agent registry so
 * adding a new CLI is a registry edit; this card
 * picks it up automatically without a UI release.
 */
export function DirectCards({
  onLaunch,
  onSetDefault,
  busy,
  defaultAgent,
  enabledAgents,
  defaultProfiles = {},
}: Props) {
  const { t } = useI18n();
  const [agents, setAgents] = useState<AgentSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [defaultBusy, setDefaultBusy] = useState<string | null>(null);
  const isDefaultDirect = Boolean(defaultAgent && !defaultProfiles[defaultAgent]);
  const enabledAgentKey = enabledAgents?.join("|") ?? "";
  const enabledAgentSet = useMemo(
    () => (enabledAgents?.length ? new Set(enabledAgents) : null),
    [enabledAgentKey],
  );

  useEffect(() => {
    void listAgents()
      .then(async (items) => {
        const desktopApps = await getDesktopAppEntriesForAgents(items);
        const rank = new Map(AGENT_DISPLAY_ORDER.map((id, index) => [id, index]));
        const installedDesktopAgents = new Set(
          Object.entries(desktopApps?.apps ?? {})
            .filter(([, app]) => app.installed)
            .map(([agentId]) => agentId),
        );
        const visible = items.filter((agent) => {
          if (agent.direct_only) {
            return installedDesktopAgents.has(agent.id);
          }
          return enabledAgentSet ? enabledAgentSet.has(agent.id) : true;
        });
        setAgents(
          [...visible].sort(
            (a, b) => (rank.get(a.id) ?? 999) - (rank.get(b.id) ?? 999),
          ),
        );
      })
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [enabledAgentSet]);

  return (
    <div
      className={`border rounded-md p-2.5 flex flex-col gap-1.5 transition-colors ${
        isDefaultDirect
          ? "border-emerald-500/70 bg-emerald-500/5 hover:border-emerald-500"
          : "border-border bg-card hover:border-primary/40"
      }`}
    >
      <div className="flex items-start gap-2">
        <span
          className="mt-0.5 h-7 w-5 shrink-0 rounded text-muted-foreground/60 inline-flex items-center justify-center"
          title={t("Direct launch stays pinned above profiles")}
        >
          <Pin className="w-3.5 h-3.5" />
        </span>
        <span className="w-7 h-7 rounded bg-primary/10 text-primary flex items-center justify-center shrink-0">
          <Sparkles className="w-3.5 h-3.5" />
        </span>
        <div className="flex-1 min-w-0">
          <div className="text-[13px] font-medium">{t("Direct launch")}</div>
          <div className="text-[11px] text-muted-foreground truncate">
            {t("No profile — uses each CLI's existing login session")}
          </div>
        </div>
      </div>
      {error && <div className="text-[11px] text-destructive">{error}</div>}
      {agents === null && !error ? (
        <div className="text-[11px] text-muted-foreground">{t("Loading…")}</div>
      ) : (
        <div className="flex flex-wrap gap-1.5 mt-1">
          {(agents ?? []).map((a) => {
            const isDefault = defaultAgent === a.id && !defaultProfiles[a.id];
            return (
              <span key={a.id} className="inline-flex h-7 overflow-hidden rounded-md bg-primary/10 text-primary">
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  onClick={() => onLaunch(a.id)}
                  disabled={busy}
                  className={`h-7 rounded-none bg-transparent px-2 text-[11px] text-primary hover:bg-primary/15 hover:text-primary ${
                    isDefault ? "" : "pr-1.5"
                  }`}
                  title={`${a.display_name} — ${a.description}`}
                >
                  <BrandIcon
                    kind="cli"
                    id={a.id}
                    label={a.display_name}
                    framed={false}
                    className="h-3.5 w-3.5"
                  />
                  {a.display_name}
                  {isDefault && <Star className="w-3 h-3 fill-current" />}
                </Button>
                {!isDefault && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    disabled={busy || defaultBusy === a.id}
                    onClick={async () => {
                      setDefaultBusy(a.id);
                      try {
                        await onSetDefault(a.id);
                      } finally {
                        setDefaultBusy(null);
                      }
                    }}
                    title={t("Use {{agent}} as Quick Launch default without a profile", {
                      agent: a.display_name,
                    })}
                    className="h-7 w-6 rounded-none border-l border-primary/15 bg-transparent text-primary/60 hover:bg-primary/15 hover:text-primary"
                  >
                    <Star className="w-3 h-3" />
                  </Button>
                )}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}
