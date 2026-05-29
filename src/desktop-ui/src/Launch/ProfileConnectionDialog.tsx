import { useMemo, useState } from "react";
import {
  AlertTriangle,
  FileText,
  Info,
  ListChecks,
  Plug,
  Plus,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { BrandIcon } from "@/components/brand-icon";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  CONNECTION_AGENTS,
  apiTypeProtocolLabel,
  apiTypeRouteLabel,
  cleanBridgeModels,
  defaultBridgeModels,
  emptyConnectionDraft,
  recommendedBridgeTarget,
  resolveProfileConnection,
} from "./connections";
import {
  HeaderSettingDialog,
  HeaderSummaryButton,
  type HeaderSetting,
} from "./ProfileConnectionHeaders";
import {
  ManualSettingDialog,
  ManualValueRow,
  PLACEHOLDER_API_KEY,
  buildManualSetting,
  manualBridgeConfig,
  type ManualSetting,
} from "./ProfileConnectionManualGuide";
import type {
  ConnectionAgentId,
  ModelDef,
  ProfileBridgeModelPreference,
  ProfileConnectionPreference,
  ProfileConnections,
  ProfileSummary,
} from "./types";

interface Props {
  profile: ProfileSummary;
  agentId: ConnectionAgentId;
  connections?: ProfileConnections;
  onClose: () => void;
  onSave: (
    agentId: ConnectionAgentId,
    preference: ProfileConnectionPreference,
  ) => Promise<void>;
}

export function ProfileConnectionDialog({
  profile,
  agentId,
  connections,
  onClose,
  onSave,
}: Props) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(() => emptyConnectionDraft(profile, connections));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [manualSetting, setManualSetting] = useState<ManualSetting | null>(null);
  const [headerSetting, setHeaderSetting] = useState<HeaderSetting | null>(null);
  const [modelSetting, setModelSetting] = useState<ModelSetting | null>(null);

  const resolved = useMemo(
    () => {
      const agent = CONNECTION_AGENTS.find((item) => item.id === agentId);
      return agent
        ? [resolveProfileConnection(profile, { [profile.id]: draft }, agent)]
        : [];
    },
    [agentId, draft, profile],
  );

  async function copyManualValue(key: string, value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedKey(key);
      window.setTimeout(() => {
        setCopiedKey((current) => (current === key ? null : current));
      }, 1400);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      await onSave(agentId, draft[agentId]);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  function updateBridgeHeaders(setting: HeaderSetting, headers: Record<string, string>) {
    setDraft((prev) => ({
      ...prev,
      [setting.agentId]: {
        ...prev[setting.agentId],
        selectedApiType:
          prev[setting.agentId].selectedApiType ?? setting.clientApiType,
        bridge: {
          ...(prev[setting.agentId].bridge ?? {}),
          [setting.clientApiType]: {
            ...(prev[setting.agentId].bridge?.[setting.clientApiType] ?? {}),
            enabled: true,
            targetApiType: setting.targetApiType,
            headers,
          },
        },
      },
    }));
  }

  function updateBridgeModels(
    setting: ModelSetting,
    models: ProfileBridgeModelPreference[],
  ) {
    const cleaned = cleanBridgeModels(models);
    const primary = cleaned[0] ?? { upstreamModel: "" };
    setDraft((prev) => ({
      ...prev,
      [setting.agentId]: {
        ...prev[setting.agentId],
        selectedApiType:
          prev[setting.agentId].selectedApiType ?? setting.clientApiType,
        bridge: {
          ...(prev[setting.agentId].bridge ?? {}),
          [setting.clientApiType]: {
            ...(prev[setting.agentId].bridge?.[setting.clientApiType] ?? {}),
            enabled: true,
            targetApiType: setting.targetApiType,
            upstreamModel: primary.upstreamModel ?? "",
            fakeModelId: primary.fakeModelId ?? undefined,
            models: cleaned,
          },
        },
      },
    }));
  }

  return (
    <>
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) {
          setManualSetting(null);
          setHeaderSetting(null);
          setModelSetting(null);
          onClose();
        }
      }}
    >
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(860px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(860px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 px-6 pt-6 pr-12">
          <DialogTitle>{t("{{label}} Connections", { label: profile.label })}</DialogTitle>
          <DialogDescription>
            {t("Choose how coding agents connect through this profile.")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-6 pb-3 [scrollbar-gutter:stable]">
          {resolved.map((connection) => {
            const { agent } = connection;
            const current = draft[agent.id];
            const selectedApiType =
              current.selectedApiType && agent.supportedApiTypes.includes(current.selectedApiType)
                ? current.selectedApiType
                : connection.selectedApiType;
            const selectedConnection =
              connection.clientApiTypes.find((item) => item.apiType === selectedApiType) ??
              connection.selected;
            const statusLabel =
              connection.status === "via_bridge"
                ? t("Via API bridge")
                : connection.status === "native"
                  ? t("Native")
                  : t("Unsupported");
            const selectedCurrentBridge = current.bridge?.[selectedApiType] ?? {};
            const selectedBridgeTarget =
              selectedCurrentBridge.targetApiType &&
              selectedConnection.targetOptions.includes(selectedCurrentBridge.targetApiType)
                ? selectedCurrentBridge.targetApiType
                : recommendedBridgeTarget(profile, agent.id, selectedApiType);
            const selectedUpstreamModel =
              cleanModelId(selectedCurrentBridge.upstreamModel) ||
              (selectedBridgeTarget ? profile.apiTypeModels[selectedBridgeTarget] : "") ||
              "";
            const selectedModelEntries = bridgeModelEntries(
              profile,
              selectedBridgeTarget,
              selectedCurrentBridge,
              selectedUpstreamModel,
            );
            const selectedCanBridge = selectedConnection.targetOptions.length > 0;
            const selectedBridgeEnabled = Boolean(
              selectedCurrentBridge.enabled && selectedCanBridge,
            );
            const handleSelectedBridgeToggle = (checked: boolean) => {
              setDraft((prev) => ({
                ...prev,
                [agent.id]: {
                  ...prev[agent.id],
                  selectedApiType:
                    prev[agent.id].selectedApiType ?? selectedApiType,
                  bridge: {
                    ...(prev[agent.id].bridge ?? {}),
                    [selectedApiType]: {
                      ...(prev[agent.id].bridge?.[selectedApiType] ?? {}),
                      enabled: checked,
                      targetApiType:
                        prev[agent.id].bridge?.[selectedApiType]?.targetApiType ??
                        selectedBridgeTarget,
                      upstreamModel:
                        prev[agent.id].bridge?.[selectedApiType]?.upstreamModel ??
                        selectedUpstreamModel,
                      models:
                        prev[agent.id].bridge?.[selectedApiType]?.models ??
                        selectedModelEntries,
                    },
                  },
                },
              }));
            };
            const authNotice = agentAuthNotice(agent.id);

            return (
              <div
                key={agent.id}
                className="rounded-md border border-border bg-card p-3"
              >
                <div className="flex items-start gap-3">
                  <BrandIcon
                    kind="cli"
                    id={agent.id}
                    label={agent.label}
                    className="h-8 w-8"
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <div className="text-[13px] font-semibold">{agent.label}</div>
                      <Badge
                        variant={connection.status === "unsupported" ? "secondary" : "default"}
                        className={
                          connection.status === "via_bridge"
                            ? "bg-primary/10 text-primary"
                            : undefined
                        }
                      >
                        {statusLabel}
                      </Badge>
                      {authNotice && connection.status !== "unsupported" && (
                        <span className="inline-flex max-w-full items-center gap-1 rounded border border-amber-500/35 bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                          <AlertTriangle className="h-3 w-3 shrink-0" />
                          <span className="truncate">{t(authNotice)}</span>
                        </span>
                      )}
                    </div>
                    <div className="mt-0.5 text-[11px] text-muted-foreground">
                      {t("Client API: {{protocol}}", {
                        protocol: apiTypeProtocolDisplayLabel(selectedApiType),
                      })}
                    </div>
                    <div className="mt-2 flex flex-wrap items-center gap-2 text-[11px]">
                      <span className="inline-flex items-center gap-1 rounded-md bg-muted px-2 py-1 text-muted-foreground">
                        <ShieldCheck className="h-3 w-3" />
                        {t("Selected route")}
                      </span>
                      <span className="font-mono text-foreground/80">
                        {selectedConnection.native
                          ? `${profile.providerLabel} · ${apiTypeProtocolDisplayLabel(selectedApiType)}`
                          : selectedConnection.bridgeEnabled && selectedConnection.targetApiType
                            ? `${apiTypeProtocolDisplayLabel(selectedApiType)} -> ${profile.providerLabel} ${apiTypeRouteDisplayLabel(selectedConnection.targetApiType)}`
                          : t("Unsupported")}
                      </span>
                    </div>
                  </div>

                  {agent.supportedApiTypes.length > 1 && (
                    <Select
                      value={selectedApiType}
                      onValueChange={(value) => {
                        setDraft((prev) => ({
                          ...prev,
                          [agent.id]: {
                            ...prev[agent.id],
                            selectedApiType: value,
                          },
                        }));
                      }}
                    >
                      <SelectTrigger size="sm" className="h-8 w-[clamp(10rem,24vw,220px)] shrink-0 text-xs">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {agent.supportedApiTypes.map((apiType) => (
                          <SelectItem key={apiType} value={apiType} className="text-xs">
                            {apiTypeProtocolDisplayLabel(apiType)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )}
                  <label className="flex shrink-0 items-center gap-2 text-[11px] text-muted-foreground">
                    {t("Enable API bridge")}
                    <Switch
                      checked={selectedBridgeEnabled}
                      disabled={!selectedCanBridge || saving}
                      onCheckedChange={handleSelectedBridgeToggle}
                    />
                  </label>
                </div>

                <div className="mt-3 grid gap-2 border-t border-border/70 pt-3">
                  {connection.clientApiTypes.map((client) => {
                    const currentBridge = current.bridge?.[client.apiType] ?? {};
                    const bridgeTarget =
                      currentBridge.targetApiType &&
                      client.targetOptions.includes(currentBridge.targetApiType)
                        ? currentBridge.targetApiType
                        : recommendedBridgeTarget(profile, agent.id, client.apiType);
                    const upstreamModel =
                      cleanModelId(currentBridge.upstreamModel) ||
                      (bridgeTarget ? profile.apiTypeModels[bridgeTarget] : "") ||
                      "";
                    const modelEntries = bridgeModelEntries(
                      profile,
                      bridgeTarget,
                      currentBridge,
                      upstreamModel,
                    );
                    const primaryModel = modelEntries[0] ?? { upstreamModel };
                    const fakeModelId = cleanModelId(primaryModel.fakeModelId);
                    const agentModel =
                      fakeModelId || cleanModelId(primaryModel.upstreamModel) || upstreamModel;
                    const upstreamModelOptions = bridgeModelOptions(
                      profile,
                      bridgeTarget,
                      upstreamModel,
                    );
                    const canBridge = client.targetOptions.length > 0;
                    const bridgeEnabled = Boolean(currentBridge.enabled && canBridge);
                    const manualConfig =
                      canBridge && bridgeTarget && agentModel
                        ? manualBridgeConfig(
                            profile.id,
                            agent.id,
                            client.apiType,
                            bridgeTarget,
                            agentModel,
                          )
                        : null;
                    if (!bridgeEnabled || !canBridge || !bridgeTarget) {
                      return null;
                    }

                    return (
                      <div
                        key={client.apiType}
                        className={`rounded-md border p-2 ${
                          selectedApiType === client.apiType
                            ? "border-primary/35 bg-primary/5"
                            : "border-border/70 bg-muted/20"
                        }`}
                      >
                        {bridgeEnabled && canBridge && bridgeTarget && (
                          <div className="grid gap-2">
                            <div className="flex items-center gap-2">
                              <Plug className="h-3.5 w-3.5 text-primary" />
                              <div className="text-[11px] font-medium">{t("Target API")}</div>
                              <div className="ml-auto">
                                <HeaderSummaryButton
                                  defaultHeaders={profile.apiTypeHeaders[bridgeTarget] ?? {}}
                                  headers={currentBridge.headers ?? {}}
                                  disabled={saving}
                                  onClick={() =>
                                    setHeaderSetting({
                                      agentId: agent.id,
                                      agentLabel: agent.label,
                                      clientApiType: client.apiType,
                                      targetApiType: bridgeTarget,
                                      defaultHeaders: profile.apiTypeHeaders[bridgeTarget] ?? {},
                                      headers: currentBridge.headers ?? {},
                                    })
                                  }
                                />
                              </div>
                              <Select
                                value={bridgeTarget}
                                onValueChange={(value) => {
                                  const nextModel = profile.apiTypeModels[value] ?? "";
                                  setDraft((prev) => ({
                                    ...prev,
                                    [agent.id]: {
                                      ...prev[agent.id],
                                      selectedApiType:
                                        prev[agent.id].selectedApiType ?? client.apiType,
                                      bridge: {
                                        ...(prev[agent.id].bridge ?? {}),
                                        [client.apiType]: {
                                          ...(prev[agent.id].bridge?.[client.apiType] ?? {}),
                                          enabled: true,
                                          targetApiType: value,
                                          upstreamModel: nextModel,
                                          fakeModelId: undefined,
                                          models: defaultBridgeModels(
                                            profile,
                                            value,
                                            nextModel,
                                          ),
                                          headers: {},
                                        },
                                      },
                                    },
                                  }));
                                }}
                              >
                                <SelectTrigger
                                  size="sm"
                                  className="!h-7 min-h-0 w-[clamp(10rem,28vw,210px)] px-2.5 py-0 text-[11px] leading-none [&_svg]:h-3.5 [&_svg]:w-3.5"
                                >
                                  <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                  {client.targetOptions.map((apiType) => (
                                    <SelectItem key={apiType} value={apiType} className="text-xs">
                                      {profile.providerLabel} ·{" "}
                                      {apiTypeProtocolDisplayLabel(apiType)}
                                    </SelectItem>
                                  ))}
                                </SelectContent>
                              </Select>
                            </div>
                            <div className="grid gap-1">
                              <div className="text-[11px] text-muted-foreground">
                                {t("Model list")}
                              </div>
                              <ModelSummaryButton
                                models={modelEntries}
                                options={upstreamModelOptions}
                                disabled={saving}
                                onClick={() =>
                                  setModelSetting({
                                    agentId: agent.id,
                                    agentLabel: agent.label,
                                    clientApiType: client.apiType,
                                    targetApiType: bridgeTarget,
                                    providerLabel: profile.providerLabel,
                                    models: modelEntries,
                                    options: upstreamModelOptions,
                                  })
                                }
                              />
                            </div>
                            <div className="font-mono text-[11px] leading-5 text-primary">
                              {apiTypeProtocolDisplayLabel(client.apiType)} -&gt;{" "}
                              {t("API bridge")} -&gt; {profile.providerLabel}{" "}
                              {apiTypeRouteDisplayLabel(bridgeTarget)}
                              {modelEntries.length > 0
                                ? ` · ${modelEntries.length === 1 ? (modelEntries[0].upstreamModel ?? "") : t("{{count}} models", { count: modelEntries.length })}`
                                : ""}
                              {fakeModelId ? ` ${t("as")} ${fakeModelId}` : ""}
                            </div>
                            {manualConfig && (
                              <div className="rounded-md border border-border/70 bg-muted/30 p-2">
                                <div className="mb-2 flex flex-wrap items-center gap-2">
                                  <div className="text-[11px] font-medium">
                                    {t("Manual setup")}
                                  </div>
                                  <Button
                                    type="button"
                                    variant="link"
                                    size="xs"
                                    className="h-auto cursor-pointer gap-1 px-0 py-0 text-[11px] font-medium"
                                    onClick={() =>
                                      setManualSetting(
                                        buildManualSetting(
                                          profile,
                                          agent.id,
                                          agent.label,
                                          client.apiType,
                                          bridgeTarget,
                                          manualConfig,
                                        ),
                                      )
                                    }
                                  >
                                    <FileText className="h-3 w-3" />
                                    {t("Setup guide")}
                                  </Button>
                                  <div className="inline-flex items-center gap-1 text-[10px] text-muted-foreground">
                                    <Info className="h-3 w-3" />
                                    {t("Click a value to copy.")}
                                  </div>
                                </div>
                                <div className="grid gap-0.5">
                                  <ManualValueRow
                                    label={t("Base URL")}
                                    value={manualConfig.baseUrl}
                                    copied={copiedKey === manualConfig.copyKey}
                                    onCopy={() =>
                                      copyManualValue(manualConfig.copyKey, manualConfig.baseUrl)
                                    }
                                  />
                                  <ManualValueRow
                                    label={t("Model")}
                                    value={manualConfig.model}
                                    copied={copiedKey === `${manualConfig.copyKey}:model`}
                                    onCopy={() =>
                                      copyManualValue(
                                        `${manualConfig.copyKey}:model`,
                                        manualConfig.model,
                                      )
                                    }
                                  />
                                  <ManualValueRow
                                    label={t("API key")}
                                    value={PLACEHOLDER_API_KEY}
                                    copied={copiedKey === `${manualConfig.copyKey}:key`}
                                    onCopy={() =>
                                      copyManualValue(
                                        `${manualConfig.copyKey}:key`,
                                        PLACEHOLDER_API_KEY,
                                      )
                                    }
                                  />
                                </div>
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </div>

        {error && <div className="shrink-0 px-6 text-[11px] text-destructive">{error}</div>}

        <DialogFooter className="shrink-0 border-t border-border px-6 py-4">
          <Button type="button" variant="outline" size="sm" onClick={onClose} disabled={saving}>
            {t("Cancel")}
          </Button>
          <Button type="button" size="sm" onClick={handleSave} disabled={saving}>
            {saving ? t("Saving…") : t("Save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    {manualSetting && (
      <ManualSettingDialog
        setting={manualSetting}
        copiedKey={copiedKey}
        onCopy={copyManualValue}
        onClose={() => setManualSetting(null)}
      />
    )}
    {headerSetting && (
      <HeaderSettingDialog
        setting={headerSetting}
        onSave={(headers) => {
          updateBridgeHeaders(headerSetting, headers);
          setHeaderSetting(null);
        }}
        onClose={() => setHeaderSetting(null)}
      />
    )}
    {modelSetting && (
      <ModelSettingDialog
        setting={modelSetting}
        onSave={(models) => {
          updateBridgeModels(modelSetting, models);
          setModelSetting(null);
        }}
        onClose={() => setModelSetting(null)}
      />
    )}
    </>
  );
}

function apiTypeProtocolDisplayLabel(apiType: string): string {
  return apiTypeProtocolLabel(apiType);
}

function apiTypeRouteDisplayLabel(apiType: string): string {
  return apiTypeRouteLabel(apiType);
}

function agentAuthNotice(agentId: ConnectionAgentId): string | null {
  switch (agentId) {
    case "claude":
      return "If Claude login overrides this profile, run claude auth logout first.";
    case "codex":
      return "If Codex login overrides this profile, run codex logout first.";
    case "gemini":
      return "If Gemini uses OAuth, run /auth and choose Gemini API key first.";
    default:
      return null;
  }
}

interface ModelSetting {
  agentId: ConnectionAgentId;
  agentLabel: string;
  clientApiType: string;
  targetApiType: string;
  providerLabel: string;
  models: ProfileBridgeModelPreference[];
  options: ModelDef[];
}

function ModelSummaryButton({
  models,
  options,
  disabled,
  onClick,
}: {
  models: ProfileBridgeModelPreference[];
  options: ModelDef[];
  disabled?: boolean;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const primary = models[0];
  const primaryLabel = primary ? modelAgentId(primary) : t("No models");
  const extra = models.length > 1 ? t("+{{count}} more", { count: models.length - 1 }) : "";
  const primaryOption = primary
    ? findModelOption(options, primary.upstreamModel ?? "")
    : null;

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      disabled={disabled}
      className="h-auto min-h-8 justify-start gap-2 px-2.5 py-1.5 text-left"
      onClick={onClick}
    >
      <ListChecks className="h-3.5 w-3.5 shrink-0 text-primary" />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-mono text-[11px]">{primaryLabel}</span>
        <span className="block truncate text-[10px] font-normal text-muted-foreground">
          {models.length === 1
            ? modelMetadataText(primaryOption, t)
            : t("{{count}} models", { count: models.length })}
          {extra ? ` · ${extra}` : ""}
        </span>
      </span>
    </Button>
  );
}

function ModelSettingDialog({
  setting,
  onSave,
  onClose,
}: {
  setting: ModelSetting;
  onSave: (models: ProfileBridgeModelPreference[]) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [models, setModels] = useState<ProfileBridgeModelPreference[]>(
    () => setting.models.length > 0 ? setting.models : [{ upstreamModel: "" }],
  );
  const datalistId = `bridge-model-options-${setting.agentId}-${setting.clientApiType}-${setting.targetApiType}`;
  const cleaned = cleanBridgeModels(models);

  function updateModel(index: number, patch: ProfileBridgeModelPreference) {
    setModels((current) =>
      current.map((model, i) => (i === index ? { ...model, ...patch } : model)),
    );
  }

  function removeModel(index: number) {
    setModels((current) =>
      current.length <= 1 ? current : current.filter((_, i) => i !== index),
    );
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(760px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(760px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 px-6 pt-6 pr-12">
          <DialogTitle>
            {t("{{agent}} model list", { agent: setting.agentLabel })}
          </DialogTitle>
          <DialogDescription>
            {t("Configure the model names exposed to the agent and the upstream model each name routes to.")}
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-h-0 flex-1 gap-2 overflow-y-auto px-6 pb-3 [scrollbar-gutter:stable]">
          <div className="rounded-md border border-border/70 bg-muted/20 p-2 text-[11px] text-muted-foreground">
            <span className="font-mono text-foreground">{setting.clientApiType}</span>
            <span> -&gt; </span>
            <span className="font-mono text-foreground">{setting.targetApiType}</span>
            <span> · {setting.providerLabel}</span>
          </div>

          <datalist id={datalistId}>
            {setting.options.map((model) => (
              <option key={model.id} value={model.id}>
                {model.label ?? model.id}
              </option>
            ))}
          </datalist>

          <div className="grid gap-2">
            {models.map((model, index) => {
              const upstreamModel = cleanModelId(model.upstreamModel);
              const option = findModelOption(setting.options, upstreamModel);
              return (
                <div
                  key={index}
                  className="grid gap-2 rounded-md border border-border/70 p-2"
                >
                  <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_28px] gap-2">
                    <label className="grid min-w-0 gap-1 text-[11px] text-muted-foreground">
                      <span>{t("Agent model id")}</span>
                      <Input
                        value={model.fakeModelId ?? ""}
                        className="h-7 w-full font-mono text-xs"
                        placeholder={upstreamModel || t("Optional")}
                        onChange={(event) =>
                          updateModel(index, { fakeModelId: event.currentTarget.value })
                        }
                      />
                    </label>
                    <label className="grid min-w-0 gap-1 text-[11px] text-muted-foreground">
                      <span>{t("Upstream model")}</span>
                      <Input
                        list={datalistId}
                        value={model.upstreamModel ?? ""}
                        className="h-7 w-full font-mono text-xs"
                        placeholder={t("model id (e.g. gpt-4o, claude-sonnet-4-6)")}
                        onChange={(event) =>
                          updateModel(index, { upstreamModel: event.currentTarget.value })
                        }
                      />
                    </label>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-sm"
                      disabled={models.length <= 1}
                      className="mt-[18px] h-7 w-7 text-muted-foreground hover:text-destructive"
                      aria-label={t("Remove model")}
                      onClick={() => removeModel(index)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                  <div className="flex flex-wrap gap-1.5 text-[10px] text-muted-foreground">
                    <span className="rounded bg-muted px-1.5 py-0.5">
                      {modelMetadataText(option, t)}
                    </span>
                    <span className="rounded bg-muted px-1.5 py-0.5">
                      {t("Agent sees {{model}}", { model: modelAgentId(model) || "..." })}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>

          <Button
            type="button"
            variant="outline"
            size="sm"
            className="w-fit gap-1.5"
            onClick={() => setModels((current) => [...current, { upstreamModel: "" }])}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("Add model")}
          </Button>
        </div>

        <DialogFooter className="shrink-0 border-t border-border px-6 py-4">
          <Button type="button" variant="outline" size="sm" onClick={onClose}>
            {t("Cancel")}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={cleaned.length === 0}
            onClick={() => onSave(cleaned)}
          >
            {t("Save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function cleanModelId(value: string | null | undefined): string {
  return value?.trim() ?? "";
}

function bridgeModelEntries(
  profile: ProfileSummary,
  targetApiType: string | null,
  bridge: { models?: ProfileBridgeModelPreference[] | null; fakeModelId?: string | null },
  upstreamModel: string,
): ProfileBridgeModelPreference[] {
  const models = cleanBridgeModels(bridge.models);
  if (models.length > 0) return models;
  return defaultBridgeModels(profile, targetApiType, upstreamModel, bridge.fakeModelId);
}

function modelAgentId(model: ProfileBridgeModelPreference): string {
  return cleanModelId(model.fakeModelId) || cleanModelId(model.upstreamModel);
}

function bridgeModelOptions(
  profile: ProfileSummary,
  targetApiType: string | null,
  currentModel: string,
): ModelDef[] {
  const options = targetApiType ? [...(profile.apiTypeModelOptions[targetApiType] ?? [])] : [];
  const model = cleanModelId(currentModel);
  if (model && !options.some((option) => option.id === model)) {
    options.unshift({ id: model, label: null });
  }
  return options;
}

function findModelOption(options: ModelDef[], model: string): ModelDef | null {
  const modelId = cleanModelId(model);
  if (!modelId) return null;
  return (
    options.find(
      (option) =>
        option.id === modelId || (option.aliases ?? []).some((alias) => alias === modelId),
    ) ?? null
  );
}

function modelMetadataText(
  option: ModelDef | null,
  t: (key: string, values?: Record<string, string | number | null | undefined>) => string,
): string {
  if (!option) return t("Custom model metadata");
  const parts = [
    option.context_window
      ? t("{{count}} context", { count: formatContextWindow(option.context_window) })
      : t("Context unknown"),
  ];
  const capabilities = [];
  if (option.capabilities?.image_input) capabilities.push(t("images"));
  if (option.capabilities?.file_input) capabilities.push(t("files"));
  if (capabilities.length > 0) parts.push(capabilities.join(", "));
  return parts.join(" · ");
}

function formatContextWindow(value: number): string {
  if (value >= 1_000_000) return `${Math.round(value / 1_000_000)}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
  return String(value);
}
