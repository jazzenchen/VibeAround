import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ChevronRight,
  ChevronLeft,
  Check,
  Rocket,
  Bot,
  MessageSquare,
  Globe,
  Sparkles,
} from "lucide-react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Settings {
  onboarded?: boolean;
  working_dir?: string;
  default_agent?: string;
  enabled_agents?: string[];
  tunnel?: {
    provider?: string;
    ngrok?: { auth_token?: string; domain?: string };
    cloudflare?: { tunnel_token?: string; hostname?: string };
  };
  channels?: {
    telegram?: {
      bot_token?: string;
      verbose?: { show_thinking?: boolean; show_tool_use?: boolean };
    };
    feishu?: {
      app_id?: string;
      app_secret?: string;
      verbose?: { show_thinking?: boolean; show_tool_use?: boolean };
    };
  };
  [key: string]: unknown;
}

const ALL_AGENTS = ["claude", "opencode", "gemini", "codex"] as const;
type AgentId = (typeof ALL_AGENTS)[number];

const AGENT_LABELS: Record<AgentId, string> = {
  claude: "Claude Code",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
  codex: "Codex",
};

const TUNNEL_PROVIDERS = ["none", "cloudflare", "ngrok"] as const;
type TunnelProvider = (typeof TUNNEL_PROVIDERS)[number];

const TUNNEL_LABELS: Record<TunnelProvider, string> = {
  none: "None (local only)",
  cloudflare: "Cloudflare Tunnel",
  ngrok: "Ngrok",
};

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

const STEPS = ["Welcome", "Agents", "Channels", "Tunnel", "Confirm"] as const;

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export default function Onboarding() {
  const [step, setStep] = useState(0);
  const [settings, setSettings] = useState<Settings>({});
  const [loaded, setLoaded] = useState(false);

  // Agent state
  const [enabledAgents, setEnabledAgents] = useState<Set<AgentId>>(
    new Set(ALL_AGENTS)
  );
  const [defaultAgent, setDefaultAgent] = useState<AgentId>("claude");

  // Channel state
  const [tgToken, setTgToken] = useState("");
  const [feishuAppId, setFeishuAppId] = useState("");
  const [feishuAppSecret, setFeishuAppSecret] = useState("");

  // Tunnel state
  const [tunnelProvider, setTunnelProvider] = useState<TunnelProvider>("none");
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokDomain, setNgrokDomain] = useState("");
  const [cfToken, setCfToken] = useState("");
  const [cfHostname, setCfHostname] = useState("");

  const [finishing, setFinishing] = useState(false);

  // Load existing settings on mount
  useEffect(() => {
    invoke<Settings>("get_settings")
      .then((s) => {
        setSettings(s);
        if (s.enabled_agents?.length) {
          setEnabledAgents(new Set(s.enabled_agents as AgentId[]));
        }
        if (s.default_agent) setDefaultAgent(s.default_agent as AgentId);
        if (s.channels?.telegram?.bot_token)
          setTgToken(s.channels.telegram.bot_token);
        if (s.channels?.feishu?.app_id)
          setFeishuAppId(s.channels.feishu.app_id);
        if (s.channels?.feishu?.app_secret)
          setFeishuAppSecret(s.channels.feishu.app_secret);
        const tp = s.tunnel?.provider;
        if (tp === "cloudflare" || tp === "ngrok") setTunnelProvider(tp);
        if (s.tunnel?.ngrok?.auth_token)
          setNgrokToken(s.tunnel.ngrok.auth_token);
        if (s.tunnel?.ngrok?.domain) setNgrokDomain(s.tunnel.ngrok.domain);
        if (s.tunnel?.cloudflare?.tunnel_token)
          setCfToken(s.tunnel.cloudflare.tunnel_token);
        if (s.tunnel?.cloudflare?.hostname)
          setCfHostname(s.tunnel.cloudflare.hostname);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, []);

  const buildSettings = useCallback((): Settings => {
    const result: Settings = {
      ...settings,
      enabled_agents: Array.from(enabledAgents),
      default_agent: defaultAgent,
    };

    // Channels
    const channels: Settings["channels"] = {};
    if (tgToken.trim()) {
      channels.telegram = {
        bot_token: tgToken.trim(),
        verbose: settings.channels?.telegram?.verbose ?? {
          show_thinking: false,
          show_tool_use: false,
        },
      };
    }
    if (feishuAppId.trim() && feishuAppSecret.trim()) {
      channels.feishu = {
        app_id: feishuAppId.trim(),
        app_secret: feishuAppSecret.trim(),
        verbose: settings.channels?.feishu?.verbose ?? {
          show_thinking: false,
          show_tool_use: false,
        },
      };
    }
    if (Object.keys(channels).length > 0) {
      result.channels = channels;
    } else {
      delete result.channels;
    }

    // Tunnel
    if (tunnelProvider !== "none") {
      const tunnel: Settings["tunnel"] = { provider: tunnelProvider };
      if (tunnelProvider === "ngrok") {
        tunnel.ngrok = {};
        if (ngrokToken.trim()) tunnel.ngrok.auth_token = ngrokToken.trim();
        if (ngrokDomain.trim()) tunnel.ngrok.domain = ngrokDomain.trim();
      }
      if (tunnelProvider === "cloudflare") {
        tunnel.cloudflare = {};
        if (cfToken.trim()) tunnel.cloudflare.tunnel_token = cfToken.trim();
        if (cfHostname.trim()) tunnel.cloudflare.hostname = cfHostname.trim();
      }
      result.tunnel = tunnel;
    } else {
      delete result.tunnel;
    }

    return result;
  }, [
    settings,
    enabledAgents,
    defaultAgent,
    tgToken,
    feishuAppId,
    feishuAppSecret,
    tunnelProvider,
    ngrokToken,
    ngrokDomain,
    cfToken,
    cfHostname,
  ]);

  const handleFinish = async () => {
    setFinishing(true);
    try {
      const final_settings = buildSettings();
      await invoke("finish_onboarding", { settings: final_settings });
      // Navigate to main dashboard
      window.location.replace("/");
    } catch (e) {
      console.error("finish_onboarding failed:", e);
      setFinishing(false);
    }
  };

  const toggleAgent = (id: AgentId) => {
    setEnabledAgents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        if (next.size > 1) next.delete(id);
      } else {
        next.add(id);
      }
      // If default agent was removed, pick first enabled
      if (!next.has(defaultAgent)) {
        setDefaultAgent(Array.from(next)[0]);
      }
      return next;
    });
  };

  if (!loaded) {
    return (
      <div className="flex items-center justify-center h-full">
        <span className="text-sm text-muted-foreground animate-pulse">
          Loading…
        </span>
      </div>
    );
  }

  const currentStep = STEPS[step];
  const isLast = step === STEPS.length - 1;
  const canNext =
    currentStep !== "Agents" || enabledAgents.size > 0;

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Progress bar */}
      <div className="flex items-center gap-1 px-6 pt-5 pb-2">
        {STEPS.map((s, i) => (
          <div key={s} className="flex items-center gap-1 flex-1">
            <div
              className={`h-1 flex-1 rounded-full transition-colors ${
                i <= step ? "bg-primary" : "bg-border"
              }`}
            />
          </div>
        ))}
      </div>
      <div className="px-6 pb-3">
        <span className="text-[10px] text-muted-foreground font-mono uppercase tracking-wider">
          Step {step + 1} of {STEPS.length} — {currentStep}
        </span>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-6 pb-4">
        {currentStep === "Welcome" && <StepWelcome />}
        {currentStep === "Agents" && (
          <StepAgents
            enabled={enabledAgents}
            defaultAgent={defaultAgent}
            onToggle={toggleAgent}
            onSetDefault={setDefaultAgent}
          />
        )}
        {currentStep === "Channels" && (
          <StepChannels
            tgToken={tgToken}
            onTgToken={setTgToken}
            feishuAppId={feishuAppId}
            onFeishuAppId={setFeishuAppId}
            feishuAppSecret={feishuAppSecret}
            onFeishuAppSecret={setFeishuAppSecret}
          />
        )}
        {currentStep === "Tunnel" && (
          <StepTunnel
            provider={tunnelProvider}
            onProvider={setTunnelProvider}
            ngrokToken={ngrokToken}
            onNgrokToken={setNgrokToken}
            ngrokDomain={ngrokDomain}
            onNgrokDomain={setNgrokDomain}
            cfToken={cfToken}
            onCfToken={setCfToken}
            cfHostname={cfHostname}
            onCfHostname={setCfHostname}
          />
        )}
        {currentStep === "Confirm" && (
          <StepConfirm
            settings={buildSettings()}
            enabledAgents={enabledAgents}
            defaultAgent={defaultAgent}
            tunnelProvider={tunnelProvider}
            hasTelegram={!!tgToken.trim()}
            hasFeishu={!!(feishuAppId.trim() && feishuAppSecret.trim())}
          />
        )}
      </div>

      {/* Footer nav */}
      <div className="flex items-center justify-between px-6 py-4 border-t border-border shrink-0">
        <button
          onClick={() => setStep((s) => Math.max(0, s - 1))}
          disabled={step === 0}
          className="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
        >
          <ChevronLeft className="w-4 h-4" />
          Back
        </button>
        {isLast ? (
          <button
            onClick={handleFinish}
            disabled={finishing}
            className="flex items-center gap-2 px-5 py-2 rounded-lg bg-primary text-primary-foreground text-sm font-medium hover:opacity-90 disabled:opacity-50 transition-opacity"
          >
            {finishing ? (
              <>Launching…</>
            ) : (
              <>
                <Rocket className="w-4 h-4" />
                Launch VibeAround
              </>
            )}
          </button>
        ) : (
          <button
            onClick={() => setStep((s) => Math.min(STEPS.length - 1, s + 1))}
            disabled={!canNext}
            className="flex items-center gap-1 px-4 py-2 rounded-lg bg-primary text-primary-foreground text-sm font-medium hover:opacity-90 disabled:opacity-50 transition-opacity"
          >
            {currentStep === "Welcome" ? "Get Started" : "Next"}
            <ChevronRight className="w-4 h-4" />
          </button>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Step components
// ---------------------------------------------------------------------------

function StepWelcome() {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-4 text-center">
      <Sparkles className="w-10 h-10 text-primary" />
      <h2 className="text-xl font-semibold">Welcome to VibeAround</h2>
      <p className="text-sm text-muted-foreground max-w-sm leading-relaxed">
        Let's set things up so you can vibe code from anywhere. This will only
        take a minute — configure your agents, messaging channels, and tunnel.
      </p>
    </div>
  );
}

function StepAgents({
  enabled,
  defaultAgent,
  onToggle,
  onSetDefault,
}: {
  enabled: Set<AgentId>;
  defaultAgent: AgentId;
  onToggle: (id: AgentId) => void;
  onSetDefault: (id: AgentId) => void;
}) {
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-semibold flex items-center gap-2">
          <Bot className="w-4 h-4 text-primary" />
          Agents
        </h2>
        <p className="text-xs text-muted-foreground mt-1">
          Choose which AI coding agents to enable. At least one is required.
        </p>
      </div>
      <div className="grid grid-cols-2 gap-2">
        {ALL_AGENTS.map((id) => {
          const isEnabled = enabled.has(id);
          const isDefault = defaultAgent === id;
          return (
            <div
              key={id}
              className={`relative flex flex-col gap-1.5 p-3 rounded-lg border cursor-pointer transition-colors ${
                isEnabled
                  ? "border-primary/40 bg-primary/5"
                  : "border-border hover:border-border/80"
              }`}
              onClick={() => onToggle(id)}
            >
              <div className="flex items-center justify-between">
                <span
                  className={`text-sm font-medium ${
                    isEnabled ? "text-foreground" : "text-muted-foreground"
                  }`}
                >
                  {AGENT_LABELS[id]}
                </span>
                <div
                  className={`w-4 h-4 rounded border flex items-center justify-center transition-colors ${
                    isEnabled
                      ? "bg-primary border-primary"
                      : "border-muted-foreground/30"
                  }`}
                >
                  {isEnabled && (
                    <Check className="w-3 h-3 text-primary-foreground" />
                  )}
                </div>
              </div>
              {isEnabled && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onSetDefault(id);
                  }}
                  className={`text-[10px] font-mono px-1.5 py-0.5 rounded self-start transition-colors ${
                    isDefault
                      ? "bg-primary text-primary-foreground"
                      : "bg-muted text-muted-foreground hover:bg-accent"
                  }`}
                >
                  {isDefault ? "★ default" : "set default"}
                </button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function StepChannels({
  tgToken,
  onTgToken,
  feishuAppId,
  onFeishuAppId,
  feishuAppSecret,
  onFeishuAppSecret,
}: {
  tgToken: string;
  onTgToken: (v: string) => void;
  feishuAppId: string;
  onFeishuAppId: (v: string) => void;
  feishuAppSecret: string;
  onFeishuAppSecret: (v: string) => void;
}) {
  return (
    <div className="space-y-5">
      <div>
        <h2 className="text-base font-semibold flex items-center gap-2">
          <MessageSquare className="w-4 h-4 text-primary" />
          IM Channels
        </h2>
        <p className="text-xs text-muted-foreground mt-1">
          Connect messaging bots to vibe code from your phone. You can skip this
          and configure later.
        </p>
      </div>

      {/* Telegram */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">Telegram</legend>
        <label className="block">
          <span className="text-xs text-muted-foreground">Bot Token</span>
          <input
            type="password"
            value={tgToken}
            onChange={(e) => onTgToken(e.target.value)}
            placeholder="123456:ABC-DEF…"
            className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
          />
        </label>
      </fieldset>

      {/* Feishu */}
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">Feishu (Lark)</legend>
        <label className="block">
          <span className="text-xs text-muted-foreground">App ID</span>
          <input
            type="text"
            value={feishuAppId}
            onChange={(e) => onFeishuAppId(e.target.value)}
            placeholder="cli_xxxx"
            className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
          />
        </label>
        <label className="block">
          <span className="text-xs text-muted-foreground">App Secret</span>
          <input
            type="password"
            value={feishuAppSecret}
            onChange={(e) => onFeishuAppSecret(e.target.value)}
            placeholder="xxxxxxxx"
            className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
          />
        </label>
      </fieldset>
    </div>
  );
}

function StepTunnel({
  provider,
  onProvider,
  ngrokToken,
  onNgrokToken,
  ngrokDomain,
  onNgrokDomain,
  cfToken,
  onCfToken,
  cfHostname,
  onCfHostname,
}: {
  provider: TunnelProvider;
  onProvider: (v: TunnelProvider) => void;
  ngrokToken: string;
  onNgrokToken: (v: string) => void;
  ngrokDomain: string;
  onNgrokDomain: (v: string) => void;
  cfToken: string;
  onCfToken: (v: string) => void;
  cfHostname: string;
  onCfHostname: (v: string) => void;
}) {
  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-semibold flex items-center gap-2">
          <Globe className="w-4 h-4 text-primary" />
          Tunnel
        </h2>
        <p className="text-xs text-muted-foreground mt-1">
          Expose your local server to the internet for IM webhooks and remote
          access. Skip if you only use it locally.
        </p>
      </div>

      <div className="flex gap-2">
        {TUNNEL_PROVIDERS.map((tp) => (
          <button
            key={tp}
            onClick={() => onProvider(tp)}
            className={`flex-1 text-xs font-medium py-2 rounded-md border transition-colors ${
              provider === tp
                ? "border-primary bg-primary/10 text-primary"
                : "border-border text-muted-foreground hover:border-border/80"
            }`}
          >
            {TUNNEL_LABELS[tp]}
          </button>
        ))}
      </div>

      {provider === "ngrok" && (
        <div className="space-y-2">
          <label className="block">
            <span className="text-xs text-muted-foreground">Auth Token</span>
            <input
              type="password"
              value={ngrokToken}
              onChange={(e) => onNgrokToken(e.target.value)}
              placeholder="2ljk…"
              className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
            />
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">
              Domain (optional)
            </span>
            <input
              type="text"
              value={ngrokDomain}
              onChange={(e) => onNgrokDomain(e.target.value)}
              placeholder="myapp.ngrok-free.app"
              className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
            />
          </label>
        </div>
      )}

      {provider === "cloudflare" && (
        <div className="space-y-2">
          <label className="block">
            <span className="text-xs text-muted-foreground">Tunnel Token</span>
            <input
              type="password"
              value={cfToken}
              onChange={(e) => onCfToken(e.target.value)}
              placeholder="eyJh…"
              className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
            />
          </label>
          <label className="block">
            <span className="text-xs text-muted-foreground">
              Hostname (optional)
            </span>
            <input
              type="text"
              value={cfHostname}
              onChange={(e) => onCfHostname(e.target.value)}
              placeholder="vibe.yourdomain.com"
              className="mt-1 w-full rounded-md border border-input bg-background px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-ring placeholder:text-muted-foreground/40"
            />
          </label>
        </div>
      )}
    </div>
  );
}

function StepConfirm({
  enabledAgents,
  defaultAgent,
  tunnelProvider,
  hasTelegram,
  hasFeishu,
}: {
  settings: Settings;
  enabledAgents: Set<AgentId>;
  defaultAgent: AgentId;
  tunnelProvider: TunnelProvider;
  hasTelegram: boolean;
  hasFeishu: boolean;
}) {
  const agents = Array.from(enabledAgents)
    .map((id) => `${AGENT_LABELS[id]}${id === defaultAgent ? " ★" : ""}`)
    .join(", ");

  const channels: string[] = [];
  if (hasTelegram) channels.push("Telegram");
  if (hasFeishu) channels.push("Feishu");

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-base font-semibold flex items-center gap-2">
          <Rocket className="w-4 h-4 text-primary" />
          Ready to Launch
        </h2>
        <p className="text-xs text-muted-foreground mt-1">
          Review your configuration. You can always change these in
          settings.json later.
        </p>
      </div>

      <div className="space-y-2 text-sm">
        <Row label="Agents" value={agents} />
        <Row
          label="Channels"
          value={channels.length > 0 ? channels.join(", ") : "None configured"}
        />
        <Row label="Tunnel" value={TUNNEL_LABELS[tunnelProvider]} />
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start gap-3 py-2 px-3 rounded-md bg-muted/40">
      <span className="text-xs text-muted-foreground w-20 shrink-0 pt-0.5">
        {label}
      </span>
      <span className="text-sm">{value}</span>
    </div>
  );
}
