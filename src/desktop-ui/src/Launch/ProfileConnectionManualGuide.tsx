import { Check, Copy } from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { API_BASE } from "@/lib/api";
import type { ConnectionAgentId, ProfileSummary } from "./types";

export const PLACEHOLDER_API_KEY = "anything-non-empty";

export interface ManualBridgeConfig {
  baseUrl: string;
  model: string;
  copyKey: string;
}

export interface ManualSetting {
  agentId: ConnectionAgentId;
  agentLabel: string;
  copyKey: string;
  filePath: string;
  profileName?: string;
  snippet: string;
}

export function manualBridgeConfig(
  profileId: string,
  agentId: ConnectionAgentId,
  clientApiType: string,
  targetApiType: string,
  model: string | undefined,
): ManualBridgeConfig {
  const path = [
    "local-api",
    encodeURIComponent(profileId),
    encodeURIComponent(`${agentId}-${clientApiType}`),
    encodeURIComponent(targetApiType),
  ].join("/");
  const versionSuffix = ["anthropic", "gemini"].includes(clientApiType) ? "" : "/v1";
  return {
    baseUrl: `${API_BASE}/${path}${versionSuffix}`,
    model: model ?? "",
    copyKey: `${agentId}:${clientApiType}:${targetApiType}:base-url`,
  };
}

export function buildManualSetting(
  profile: ProfileSummary,
  agentId: ConnectionAgentId,
  agentLabel: string,
  clientApiType: string,
  targetApiType: string,
  manualConfig: ManualBridgeConfig,
): ManualSetting {
  const model = manualConfig.model || "<model-id>";
  if (agentId === "codex" || agentId === "codex-desktop") {
    const profileName = codexProfileName(profile.id, targetApiType);
    const providerName = profileName;
    return {
      agentId,
      agentLabel,
      copyKey: `${manualConfig.copyKey}:codex-config`,
      filePath: "~/.codex/config.toml",
      profileName,
      snippet: [
        `model = ${tomlString(model)}`,
        `model_provider = ${tomlString(providerName)}`,
        `model_reasoning_effort = "medium"`,
        "",
        `[model_providers.${providerName}]`,
        `name = ${tomlString(`VibeAround ${profile.providerLabel}`)}`,
        `base_url = ${tomlString(manualConfig.baseUrl)}`,
        `wire_api = "responses"`,
        `requires_openai_auth = false`,
      ].join("\n"),
    };
  }

  if (agentId === "claude-desktop") {
    return {
      agentId,
      agentLabel,
      copyKey: `${manualConfig.copyKey}:claude-desktop-3p`,
      filePath: "Claude Desktop third-party inference profile",
      snippet: JSON.stringify(
        {
          inferenceProvider: "gateway",
          inferenceGatewayBaseUrl: manualConfig.baseUrl,
          inferenceGatewayApiKey: PLACEHOLDER_API_KEY,
          inferenceGatewayAuthScheme: "bearer",
          modelDiscoveryEnabled: true,
        },
        null,
        2,
      ),
    };
  }

  if (agentId === "opencode") {
    const npm =
      clientApiType === "anthropic"
        ? "@ai-sdk/anthropic"
        : clientApiType === "openai-chat"
          ? "@ai-sdk/openai-compatible"
          : "@ai-sdk/openai";
    return {
      agentId,
      agentLabel,
      copyKey: `${manualConfig.copyKey}:opencode-config`,
      filePath: "~/.config/opencode/opencode.json",
      snippet: JSON.stringify(
        {
          $schema: "https://opencode.ai/config.json",
          model: `${profile.provider}/${model}`,
          provider: {
            [profile.provider]: {
              npm,
              name: `VibeAround ${profile.providerLabel}`,
              options: {
                baseURL: manualConfig.baseUrl,
                apiKey: PLACEHOLDER_API_KEY,
                setCacheKey: true,
              },
              models: {
                [model]: { name: model },
              },
            },
          },
        },
        null,
        2,
      ),
    };
  }

  if (agentId === "gemini") {
    return {
      agentId,
      agentLabel,
      copyKey: `${manualConfig.copyKey}:gemini-env`,
      filePath: "~/.gemini/settings.json + ~/.gemini/.env",
      snippet: [
        `// ~/.gemini/settings.json`,
        `{`,
        `  "security": {`,
        `    "auth": {`,
        `      "selectedType": "gemini-api-key"`,
        `    }`,
        `  }`,
        `}`,
        ``,
        `# ~/.gemini/.env`,
        `GEMINI_API_KEY=${PLACEHOLDER_API_KEY}`,
        `GOOGLE_API_KEY=${PLACEHOLDER_API_KEY}`,
        `GEMINI_DEFAULT_AUTH_TYPE=gemini-api-key`,
        `GOOGLE_GEMINI_BASE_URL=${manualConfig.baseUrl}`,
        `GEMINI_MODEL=${model}`,
      ].join("\n"),
    };
  }

  if (agentId === "pi") {
    const providerName = piProviderName(profile.id, clientApiType);
    const providerConfig = JSON.stringify(
      {
        name: `VibeAround ${profile.providerLabel}`,
        baseUrl: manualConfig.baseUrl,
        apiKey: PLACEHOLDER_API_KEY,
        api: piApiName(clientApiType),
        models: [{ id: model, name: model }],
      },
      null,
      2,
    ).replace(/\n/g, "\n  ");
    return {
      agentId,
      agentLabel,
      copyKey: `${manualConfig.copyKey}:pi-extension`,
      filePath: "Pi extension file used with pi --extension <path>",
      snippet: [
        "export default function (pi) {",
        `  pi.registerProvider(${JSON.stringify(providerName)}, ${providerConfig});`,
        "}",
      ].join("\n"),
    };
  }

  const claudeEnv: Record<string, string> = {
    ANTHROPIC_API_KEY: PLACEHOLDER_API_KEY,
    ANTHROPIC_AUTH_TOKEN: PLACEHOLDER_API_KEY,
    ANTHROPIC_BASE_URL: manualConfig.baseUrl,
    ANTHROPIC_MODEL: model,
  };

  return {
    agentId,
    agentLabel,
    copyKey: `${manualConfig.copyKey}:claude-settings`,
    filePath: "~/.claude/settings.json",
    snippet: `"env": ${JSON.stringify(claudeEnv, null, 2)}`,
  };
}

export function ManualSettingDialog({
  setting,
  copiedKey,
  onCopy,
  onClose,
}: {
  setting: ManualSetting;
  copiedKey: string | null;
  onCopy: (key: string, value: string) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const isCodex = setting.agentId === "codex" || setting.agentId === "codex-desktop";
  const isDesktop = setting.agentId === "claude-desktop" || setting.agentId === "codex-desktop";
  const isOpenCode = setting.agentId === "opencode";
  const isGemini = setting.agentId === "gemini";
  const isPi = setting.agentId === "pi";

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(700px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(700px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 px-6 pt-6 pr-12">
          <DialogTitle>
            {t("{{agent}} manual setting", { agent: setting.agentLabel })}
          </DialogTitle>
          <DialogDescription>
            {isGemini
              ? t("Copy the matching parts into the Gemini CLI settings and env files yourself. VibeAround does not edit them automatically.")
              : isPi
                ? t("Copy this extension into a local file and pass it to pi with --extension yourself. VibeAround does not edit Pi global config automatically.")
              : isDesktop
                ? t("VibeAround writes this desktop configuration during launch. Copy this only if you are configuring the desktop app by hand.")
              : t("Copy this snippet into the CLI config file yourself. VibeAround does not edit the file automatically.")}
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-h-0 flex-1 gap-3 overflow-y-auto px-6 pb-6 [scrollbar-gutter:stable]">
          <div className="grid gap-2 rounded-md border border-border/70 bg-muted/25 p-3 text-[12px]">
            <ConfigInfoRow label={t("Configuration file")} value={setting.filePath} />
          </div>

          <div className="rounded-md border border-border/70 p-3">
            <div className="text-[12px] font-medium">{t("How to modify")}</div>
            <ol className="mt-2 space-y-1.5 pl-4 text-[12px] leading-relaxed text-muted-foreground">
              {setting.agentId === "claude-desktop" ? (
                <>
                  <li>{t("Open Claude Desktop's third-party inference profile settings.")}</li>
                  <li>{t("Use the local bridge base URL and any non-empty API key value.")}</li>
                  <li>{t("Keep model discovery enabled so Claude Desktop can list the bridge models.")}</li>
                </>
              ) : isCodex ? (
                <>
                  <li>{t("Open the Codex config file, then add this snippet or update the existing VibeAround top-level model settings.")}</li>
                  <li>{t("The top-level model and provider lines make Codex use this VibeAround bridge by default.")}</li>
                  <li>{t("If Codex keeps using account login instead of this bridge config, run codex logout first.")}</li>
                </>
              ) : isOpenCode ? (
                <>
                  <li>{t("Open the OpenCode config file, then add or merge this provider block.")}</li>
                  <li>{t("Use any non-empty API key value when the local API bridge is already running with a saved profile key.")}</li>
                </>
              ) : isGemini ? (
                <>
                  <li>{t("Open the Gemini CLI settings file and make sure selectedType is gemini-api-key.")}</li>
                  <li>{t("Open the Gemini CLI env file, then add or update these variables.")}</li>
                  <li>{t("If Gemini keeps using OAuth, run /auth in Gemini CLI and choose Gemini API key.")}</li>
                  <li>{t("Use any non-empty API key value when the local API bridge is already running with a saved profile key.")}</li>
                </>
              ) : isPi ? (
                <>
                  <li>{t("Save this snippet as a local Pi extension file.")}</li>
                  <li>{t("Start Pi with --extension pointing at that file, plus the provider and model shown in the snippet.")}</li>
                  <li>{t("Use any non-empty API key value when the local API bridge is already running with a saved profile key.")}</li>
                </>
              ) : (
                <>
                  <li>{t("Paste this property inside the root JSON object of Claude settings.")}</li>
                  <li>{t("If env already exists, merge these keys into the existing env object instead of creating another env block.")}</li>
                  <li>{t("If Claude keeps using account login instead of this env block, run claude auth logout first.")}</li>
                </>
              )}
            </ol>
          </div>

          <ConfigSnippetBlock
            title={
              setting.agentId === "claude-desktop"
                ? t("Claude Desktop profile snippet")
                : isCodex
                ? t("Codex config snippet")
                : isOpenCode
                  ? t("OpenCode config snippet")
                  : isGemini
                    ? t("Gemini config snippet")
                    : isPi
                      ? t("Pi extension snippet")
                  : t("Config snippet")
            }
            snippet={setting.snippet}
            copied={copiedKey === setting.copyKey}
            onCopy={() => onCopy(setting.copyKey, setting.snippet)}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ConfigSnippetBlock({
  title,
  snippet,
  copied,
  onCopy,
}: {
  title: string;
  snippet: string;
  copied: boolean;
  onCopy: () => void;
}) {
  const { t } = useI18n();

  return (
    <div
      className={`overflow-hidden rounded-md border ${
        copied
          ? "border-primary/60 bg-primary/10"
          : "border-primary/30 bg-primary/5"
      }`}
    >
      <div className="flex items-center justify-between gap-2 border-b border-primary/20 px-3 py-2">
        <div className="text-[12px] font-medium text-primary">{title}</div>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="h-6 gap-1.5 px-2 text-[11px] font-medium text-primary hover:bg-primary/10 hover:text-primary"
          onClick={onCopy}
        >
          {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
          {copied ? t("Copied") : t("Copy config")}
        </Button>
      </div>
      <pre className="max-h-[280px] overflow-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-[11px] leading-relaxed text-foreground">
        {snippet}
      </pre>
    </div>
  );
}

function ConfigInfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <div className="shrink-0 text-muted-foreground">{label}</div>
      <div className="break-all font-mono text-foreground">{value}</div>
    </div>
  );
}

export function ManualValueRow({
  label,
  value,
  copied,
  onCopy,
}: {
  label: string;
  value: string;
  copied: boolean;
  onCopy: () => void;
}) {
  const { t } = useI18n();

  return (
    <div className="grid grid-cols-[56px_minmax(0,1fr)] items-center gap-1">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <button
        type="button"
        className={`group flex min-w-0 cursor-pointer items-center rounded px-0.5 py-0 text-left font-mono text-[11px] leading-5 transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring ${
          copied
            ? "bg-primary/10 text-primary"
            : "text-foreground hover:bg-primary/5 hover:text-primary"
        }`}
        onClick={onCopy}
        title={value}
      >
        <span className="min-w-0 flex-1 truncate">{value}</span>
        {copied && (
          <span className="ml-1.5 inline-flex shrink-0 items-center gap-1 text-[10px] font-sans">
            <Check className="h-3 w-3" />
            {t("Copied")}
          </span>
        )}
      </button>
    </div>
  );
}

function codexProfileName(profileId: string, targetApiType: string): string {
  return `vibearound_${safeConfigKey(profileId)}_${safeConfigKey(targetApiType)}`;
}

function safeConfigKey(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "") || "profile";
}

function tomlString(value: string): string {
  return JSON.stringify(value);
}

function piProviderName(profileId: string, clientApiType: string): string {
  return `vibearound-${safePiId(profileId)}-${safePiId(clientApiType)}`;
}

function safePiId(value: string): string {
  return (
    value
      .trim()
      .replace(/[^A-Za-z0-9_-]+/g, "_")
      .replace(/^_+|_+$/g, "") || "profile"
  );
}

function piApiName(apiType: string): string {
  switch (apiType) {
    case "anthropic":
      return "anthropic-messages";
    case "openai-responses":
      return "openai-responses";
    default:
      return "openai-completions";
  }
}
