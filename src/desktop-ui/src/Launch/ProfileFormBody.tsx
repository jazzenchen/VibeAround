import { useEffect, useState, type ReactNode } from "react";

import {
  AlertCircle,
  CheckCircle2,
  Eye,
  EyeOff,
  FlaskConical,
  Globe,
  ListChecks,
  Loader2,
  LogIn,
  Plus,
  Star,
  Trash2,
} from "lucide-react";
import { useI18n } from "@va/i18n";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  INPUT_CLASS,
  MONO_INPUT_CLASS,
  SECRET_INPUT_CLASS,
} from "./ProfileFormDialog.constants";
import {
  googleOAuthLogin,
  googleOAuthStatus,
  type GoogleOAuthStatus,
} from "./api";
import {
  arraysEqual,
  apiConfigForEndpoint,
  apiConfigsForEndpoints,
  collectFields,
  defaultAuthMode,
  endpointId,
  endpointLabel,
  endpointsForApiType,
  providerEndpointGroups,
  providerApiKindsEditable,
  providerApiKindEndpoints,
  providerUsesEndpointGroups,
  requiresProfileModel,
  selectedEndpointGroup,
  selectedAuthModes,
  selectedEndpoint,
  shouldShowBaseUrl,
  syncApiConfigsForProvider,
} from "./profileFormHelpers";
import type {
  AuthMode,
  CatalogEntry,
  FieldDef,
  ProfileApiConfig,
  ProfileModelConfig,
} from "./types";
import type { ProviderSettings } from "./types";
import { apiTypeLabel, apiTypeShort } from "./types";

type ModelTestOutcome = { ok: boolean; message: string; url?: string };
type ModelTestStatus =
  | { state: "idle" }
  | { state: "testing" }
  | { state: "success"; message: string; url?: string }
  | { state: "error"; message: string };

interface FormBodyProps {
  provider: CatalogEntry;
  label: string;
  setLabel: (v: string) => void;
  selectedApiTypes: string[];
  setSelectedApiTypes: (v: string[]) => void;
  authMode: AuthMode;
  setAuthMode: (v: AuthMode) => void;
  credentials: Record<string, string>;
  setCredentials: (v: Record<string, string>) => void;
  apiConfigs: Record<string, ProfileApiConfig>;
  setApiConfigs: (v: Record<string, ProfileApiConfig>) => void;
  useSettingsProxy: boolean;
  setUseSettingsProxy: (v: boolean) => void;
  providerSettings: ProviderSettings;
  setProviderSettings: (v: ProviderSettings) => void;
  revealKeys: Record<string, boolean>;
  setRevealKeys: (v: Record<string, boolean>) => void;
  testingDisabled?: boolean;
  onTestModel: (apiType: string, model: string) => Promise<ModelTestOutcome>;
}

export function FormBody({
  provider,
  label,
  setLabel,
  selectedApiTypes,
  setSelectedApiTypes,
  authMode,
  setAuthMode,
  credentials,
  setCredentials,
  apiConfigs,
  setApiConfigs,
  useSettingsProxy,
  setUseSettingsProxy,
  providerSettings,
  setProviderSettings,
  revealKeys,
  setRevealKeys,
  testingDisabled,
  onTestModel,
}: FormBodyProps) {
  const { t } = useI18n();
  const endpointGroups = providerEndpointGroups(provider);
  const usesEndpointGroups = providerUsesEndpointGroups(provider);
  const selectedGroup = usesEndpointGroups
    ? selectedEndpointGroup(provider, selectedApiTypes, apiConfigs)
    : undefined;
  const apiKindEndpoints = providerApiKindEndpoints(provider);
  const visibleApiKindEndpoints = selectedGroup?.endpoints ?? apiKindEndpoints;
  const visibleApiTypes = visibleApiKindEndpoints.map((endpoint) => endpoint.api_type);
  const visibleApiTypeSet = new Set(visibleApiTypes);
  const effectiveSelectedApiTypes = usesEndpointGroups
    ? selectedApiTypes.filter((apiType) => visibleApiTypeSet.has(apiType))
    : selectedApiTypes;
  const fieldDefs = collectFields(
    provider,
    effectiveSelectedApiTypes,
    authMode,
    apiConfigs,
  );
  const [googleStatus, setGoogleStatus] = useState<GoogleOAuthStatus | null>(null);
  const [googleLoading, setGoogleLoading] = useState(false);
  const [googleError, setGoogleError] = useState<string | null>(null);
  const [modelsDialogApiType, setModelsDialogApiType] = useState<string | null>(null);
  const [selectedTestModels, setSelectedTestModels] = useState<Record<string, string[]>>({});
  const [modelTestStatus, setModelTestStatus] = useState<Record<string, ModelTestStatus>>({});
  const authModeOptions = selectedAuthModes(
    provider,
    effectiveSelectedApiTypes,
    apiConfigs,
  );
  const googleAccountsSelected =
    provider.id === "gemini" &&
    (authMode === "google_oauth" || authMode === "oauth_via_cli") &&
    effectiveSelectedApiTypes.some((apiType) => {
      const endpoint = selectedEndpoint(provider, apiType, apiConfigs);
      return endpoint ? endpointId(endpoint) === "google-accounts" : false;
    });
  const apiKindsEditable = providerApiKindsEditable(provider);
  const configurableApiTypes = effectiveSelectedApiTypes.filter((apiType) => {
    const ep = selectedEndpoint(provider, apiType, apiConfigs);
    return !!ep;
  });

  useEffect(() => {
    if (!usesEndpointGroups || !selectedGroup) return;
    const next = apiKindsEditable
      ? effectiveSelectedApiTypes.length > 0
        ? effectiveSelectedApiTypes
        : visibleApiTypes
      : visibleApiTypes;
    if (!arraysEqual(selectedApiTypes, next)) {
      setSelectedApiTypes(next);
    }
  }, [
    apiKindsEditable,
    effectiveSelectedApiTypes,
    selectedApiTypes,
    selectedGroup,
    setSelectedApiTypes,
    usesEndpointGroups,
    visibleApiTypes,
  ]);

  useEffect(() => {
    let cancelled = false;
    if (!googleAccountsSelected) {
      setGoogleStatus(null);
      setGoogleError(null);
      setGoogleLoading(false);
      return;
    }

    setGoogleError(null);
    googleOAuthStatus()
      .then((status) => {
        if (!cancelled) setGoogleStatus(status);
      })
      .catch((error) => {
        if (!cancelled) {
          setGoogleError(error instanceof Error ? error.message : String(error));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [googleAccountsSelected]);

  async function handleGoogleOAuthLogin() {
    setGoogleLoading(true);
    setGoogleError(null);
    try {
      setGoogleStatus(await googleOAuthLogin());
    } catch (error) {
      setGoogleError(error instanceof Error ? error.message : String(error));
    } finally {
      setGoogleLoading(false);
    }
  }

  function applyEndpointGroup(groupId: string) {
    const group = endpointGroups.find((candidate) => candidate.id === groupId);
    if (!group) return;
    const nextApiTypes = group.endpoints.map((endpoint) => endpoint.api_type);
    const nextApiConfigs = syncApiConfigsForProvider(
      provider,
      nextApiTypes,
      apiConfigsForEndpoints(group.endpoints, apiConfigs),
    );
    setSelectedApiTypes(nextApiTypes);
    setApiConfigs(nextApiConfigs);
    setAuthMode(defaultAuthMode(provider, nextApiTypes, nextApiConfigs, authMode));
  }

  function applySelectedApiTypes(apiTypes: string[]) {
    const nextApiConfigs = { ...apiConfigs };
    for (const endpoint of visibleApiKindEndpoints) {
      const currentConfig = nextApiConfigs[endpoint.api_type] ?? {};
      nextApiConfigs[endpoint.api_type] = {
        ...apiConfigForEndpoint(endpoint, currentConfig),
        enabled: apiTypes.includes(endpoint.api_type),
      };
    }
    setApiConfigs(nextApiConfigs);
    setSelectedApiTypes(apiTypes);
    setAuthMode(defaultAuthMode(provider, apiTypes, nextApiConfigs, authMode));
  }

  function updateApiConfig(
    apiType: string,
    updater: (config: ProfileApiConfig) => ProfileApiConfig,
  ) {
    setApiConfigs({
      ...apiConfigs,
      [apiType]: updater(apiConfigs[apiType] ?? { enabled: true }),
    });
  }

  function modelDialogKey(apiType: string, endpoint: CatalogEntry["endpoints"][number]) {
    return `${apiType}:${endpointId(endpoint)}`;
  }

  function modelStatusKey(
    apiType: string,
    endpoint: CatalogEntry["endpoints"][number],
    model: string,
  ) {
    return `${modelDialogKey(apiType, endpoint)}:${model}`;
  }

  function openModelsDialog(
    apiType: string,
    endpoint: CatalogEntry["endpoints"][number],
    selectedModel: string,
  ) {
    const key = modelDialogKey(apiType, endpoint);
    setSelectedTestModels((current) => {
      if (current[key]?.length) return current;
      const config = apiConfigForEndpoint(endpoint, apiConfigs[apiType]);
      const enabledModels = (config.models ?? [])
        .filter((model) => model.enabled !== false)
        .map((model) => model.id.trim())
        .filter(Boolean);
      const initialModels = selectedModel
        ? Array.from(new Set([selectedModel, ...enabledModels]))
        : enabledModels.length > 0
          ? enabledModels
          : endpoint.models[0]?.id
            ? [endpoint.models[0].id]
            : [];
      return { ...current, [key]: initialModels };
    });
    setModelsDialogApiType(apiType);
  }

  async function testSelectedModels(
    apiType: string,
    endpoint: CatalogEntry["endpoints"][number],
  ) {
    const key = modelDialogKey(apiType, endpoint);
    const models = selectedTestModels[key] ?? [];
    for (const model of models) {
      const statusKey = modelStatusKey(apiType, endpoint, model);
      setModelTestStatus((current) => ({
        ...current,
        [statusKey]: { state: "testing" },
      }));
      const result = await onTestModel(apiType, model);
      setModelTestStatus((current) => ({
        ...current,
        [statusKey]: result.ok
          ? { state: "success", message: result.message, url: result.url }
          : { state: "error", message: result.message },
      }));
    }
  }

  const modelsDialogEndpoint = modelsDialogApiType
    ? selectedEndpoint(provider, modelsDialogApiType, apiConfigs)
    : null;
  const modelsDialogStoredConfig = modelsDialogApiType
    ? (apiConfigs[modelsDialogApiType] ?? {})
    : {};
  const modelsDialogConfig =
    modelsDialogApiType && modelsDialogEndpoint
      ? apiConfigForEndpoint(
          modelsDialogEndpoint,
          apiConfigs[modelsDialogApiType],
        )
      : null;
  const modelsDialogSelectedModel =
    modelsDialogApiType && modelsDialogEndpoint
      ? modelsDialogConfig?.model?.trim() ||
        modelsDialogEndpoint.models[0]?.id ||
        ""
      : "";

  function updateModelsDialogConfig(
    nextModels: ProfileModelConfig[],
    preferredDefaultModel?: string,
  ) {
    if (!modelsDialogApiType || !modelsDialogEndpoint) return;
    const enabledIds = enabledProfileModelIds(nextModels);
    const preferred = preferredDefaultModel?.trim() || modelsDialogSelectedModel.trim();
    const defaultModel =
      preferred && enabledIds.includes(preferred)
        ? preferred
        : enabledIds[0] || preferred;
    const catalogDefaultModel = modelsDialogEndpoint.models[0]?.id ?? "";
    const selectedModel = !defaultModel || (
      !requiresProfileModel(provider, modelsDialogEndpoint) &&
      defaultModel === catalogDefaultModel
    ) ? undefined : defaultModel;
    updateApiConfig(modelsDialogApiType, (config) => ({
      ...apiConfigForEndpoint(modelsDialogEndpoint, config),
      model: selectedModel,
      models: nextModels,
    }));
    setSelectedTestModels((current) => ({
      ...current,
      [modelDialogKey(modelsDialogApiType, modelsDialogEndpoint)]: enabledIds,
    }));
  }

  function updateModelsDialogDefault(model: string) {
    if (!modelsDialogConfig) return;
    updateModelsDialogConfig(ensureProfileModel(modelsDialogConfig.models ?? [], model), model);
  }

  function updateModelsDialogEnabled(modelIds: string[]) {
    if (!modelsDialogConfig) return;
    updateModelsDialogConfig(
      setProfileModelsEnabled(modelsDialogConfig.models ?? [], modelIds),
    );
  }

  return (
    <div className="space-y-3">
      <FormSection title={t("Profile")}>
        <FieldRow label={t("Label")}>
          <Input
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder={`${provider.label} (work)`}
            className={INPUT_CLASS}
          />
        </FieldRow>

        {usesEndpointGroups && selectedGroup && (
          <EndpointGroupField
            groups={endpointGroups}
            selectedGroupId={selectedGroup.id}
            onChange={applyEndpointGroup}
          />
        )}

        <ApiKindsField
          endpoints={visibleApiKindEndpoints}
          editable={apiKindsEditable}
          selectedApiTypes={effectiveSelectedApiTypes}
          setSelectedApiTypes={applySelectedApiTypes}
          onOpenModels={(endpoint) => {
            const selectedModel =
              apiConfigs[endpoint.api_type]?.model?.trim() ||
              endpoint.models[0]?.id ||
              "";
            openModelsDialog(endpoint.api_type, endpoint, selectedModel);
          }}
        />

        {effectiveSelectedApiTypes.length > 0 && authModeOptions.length > 1 && (
          <FieldRow label={t("Auth method")}>
            <Select
              value={authMode}
              onValueChange={(value) =>
                setAuthMode(
                  defaultAuthMode(
                    provider,
                    effectiveSelectedApiTypes,
                    apiConfigs,
                    value as AuthMode,
                  ),
                )
              }
            >
              <SelectTrigger size="sm" className="h-8 w-full text-[13px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {authModeOptions.map((auth) => (
                  <SelectItem
                    key={auth.mode}
                    value={auth.mode}
                    className="text-xs"
                  >
                    {t(auth.label ?? auth.mode)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FieldRow>
        )}

        {effectiveSelectedApiTypes.length > 0 && googleAccountsSelected && (
          <GoogleOAuthField
            status={googleStatus}
            loading={googleLoading}
            error={googleError}
            onLogin={handleGoogleOAuthLogin}
          />
        )}

        {effectiveSelectedApiTypes.length > 0 &&
          fieldDefs.map((f) => (
            <CredentialField
              key={f.name}
              field={f}
              value={credentials[f.name] ?? ""}
              reveal={revealKeys[f.name] ?? false}
              onChange={(v) => setCredentials({ ...credentials, [f.name]: v })}
              onToggleReveal={() =>
                setRevealKeys({ ...revealKeys, [f.name]: !revealKeys[f.name] })
              }
            />
          ))}
      </FormSection>

      {effectiveSelectedApiTypes.length > 0 && (
        <FormSection title={t("Endpoint settings")}>
          {configurableApiTypes.length > 0 && (
            <div className="space-y-2">
              {configurableApiTypes.map((apiType) => {
                const ep = selectedEndpoint(provider, apiType, apiConfigs);
                if (!ep) return null;
                const apiConfig = apiConfigForEndpoint(ep, apiConfigs[apiType]);
                const endpointOptions = endpointsForApiType(provider, apiType);
                return (
                  <div
                    key={apiType}
                    className="border border-border/60 rounded-md p-2.5 space-y-2"
                  >
                    <div className="flex items-center justify-between gap-2 text-xs">
                      <div className="flex min-w-0 items-center gap-2">
                        <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 font-mono">
                          {apiTypeShort(apiType)}
                        </span>
                        <span className="truncate text-muted-foreground/70">
                          · {t(apiTypeLabel(apiType))}
                        </span>
                      </div>
                    </div>
                    {!usesEndpointGroups && endpointOptions.length > 1 && (
                      <FieldRow label={t("Endpoint type")}>
                        <Select
                          value={endpointId(ep)}
                          onValueChange={(value) => {
                            const nextEndpoint =
                              endpointOptions.find(
                                (endpoint) => endpointId(endpoint) === value,
                              ) ?? endpointOptions[0];
                            if (!nextEndpoint) return;
                            setApiConfigs(
                              apiConfigsForEndpoints(
                                [nextEndpoint],
                                apiConfigs,
                              ),
                            );
                          }}
                        >
                          <SelectTrigger
                            size="sm"
                            className="h-8 w-full text-[13px]"
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {endpointOptions.map((endpoint) => (
                              <SelectItem
                                key={endpointId(endpoint)}
                                value={endpointId(endpoint)}
                                className="text-xs"
                              >
                                {t(endpointLabel(endpoint))}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </FieldRow>
                    )}
                    {shouldShowBaseUrl(provider, ep, apiConfig) && (
                      <FieldRow
                        label={
                          provider.id === "azure" || provider.id === "gemini"
                            ? "Endpoint"
                            : "Base URL"
                        }
                        required={ep.default_base_url === ""}
                        hint={t(
                          "Enter it as your provider documents it, usually ending in /v1. Some clients append /v1 themselves — if requests fail on a doubled /v1 path, drop the trailing /v1 here.",
                        )}
                      >
                        <Input
                          type="text"
                          value={apiConfig.base_url ?? ""}
                          onChange={(e) => {
                            const value = e.target.value;
                            updateApiConfig(apiType, (config) => ({
                              ...config,
                              base_url: value,
                            }));
                          }}
                          placeholder={
                            ep.default_base_url ||
                            (provider.id === "azure"
                              ? "https://your-resource.openai.azure.com/openai/v1"
                              : provider.id === "gemini"
                                ? "https://aiplatform.googleapis.com/v1/projects/PROJECT/locations/LOCATION/endpoints/openapi"
                              : "https://your-endpoint.example.com/v1")
                          }
                          className={MONO_INPUT_CLASS}
                        />
                      </FieldRow>
                    )}
                  </div>
                );
              })}
            </div>
          )}

          <ProxyField
            checked={useSettingsProxy}
            onChange={setUseSettingsProxy}
          />

          {modelsDialogApiType && modelsDialogEndpoint && (
            <ModelCatalogDialog
              provider={provider}
              apiType={modelsDialogApiType}
              endpoint={modelsDialogEndpoint}
              baseUrl={
                modelsDialogStoredConfig.base_url?.trim() ||
                modelsDialogConfig?.base_url?.trim() ||
                modelsDialogEndpoint.default_base_url
              }
              models={modelsDialogConfig?.models ?? []}
              selectedModel={modelsDialogSelectedModel}
              checkedModels={
                selectedTestModels[
                  modelDialogKey(modelsDialogApiType, modelsDialogEndpoint)
                ] ?? []
              }
              statuses={modelTestStatus}
              statusKeyFor={(model) =>
                modelStatusKey(modelsDialogApiType, modelsDialogEndpoint, model)
              }
              testingDisabled={!!testingDisabled}
              onDefaultModelChange={updateModelsDialogDefault}
              onCheckedModelsChange={updateModelsDialogEnabled}
              onModelsChange={updateModelsDialogConfig}
              onTestSelected={() =>
                void testSelectedModels(modelsDialogApiType, modelsDialogEndpoint)
              }
              onClose={() => setModelsDialogApiType(null)}
            />
          )}
        </FormSection>
      )}

      {provider.id === "deepseek" && selectedApiTypes.includes("openai-chat") && (
        <FormSection title={t("DeepSeek API bridge")}>
          <DeepSeekBridgeSettingsField
            settings={providerSettings}
            onChange={setProviderSettings}
          />
        </FormSection>
      )}

      {provider.homepage && (
        <a
          href={provider.homepage}
          target="_blank"
          rel="noopener noreferrer"
          className="text-[11px] text-primary hover:underline flex items-center gap-1"
        >
          <Globe className="w-3 h-3" /> {provider.homepage}
        </a>
      )}
    </div>
  );
}

function EndpointGroupField({
  groups,
  selectedGroupId,
  onChange,
}: {
  groups: ReturnType<typeof providerEndpointGroups>;
  selectedGroupId: string;
  onChange: (groupId: string) => void;
}) {
  const { t } = useI18n();

  return (
    <FieldRow label={t("Endpoint")}>
      <Select value={selectedGroupId} onValueChange={onChange}>
        <SelectTrigger size="sm" className="h-8 w-full text-[13px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {groups.map((group) => (
            <SelectItem key={group.id} value={group.id} className="text-xs">
              {t(group.label)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </FieldRow>
  );
}

function ensureProfileModel(
  models: ProfileModelConfig[] | null | undefined,
  modelId: string,
): ProfileModelConfig[] {
  const id = modelId.trim();
  const next = models?.length ? [...models] : [];
  if (!id) return next;
  const existingIndex = next.findIndex((model) => model.id === id);
  if (existingIndex >= 0) {
    next[existingIndex] = { ...next[existingIndex], enabled: true };
    return next;
  }
  return [
    {
      id,
      enabled: true,
      capabilities: {},
      custom: true,
    },
    ...next,
  ];
}

function enabledProfileModelIds(models: ProfileModelConfig[]): string[] {
  return models
    .filter((model) => model.enabled !== false)
    .map((model) => model.id.trim())
    .filter(Boolean);
}

function setProfileModelsEnabled(
  models: ProfileModelConfig[],
  enabledIds: string[],
): ProfileModelConfig[] {
  const enabled = new Set(enabledIds.map((id) => id.trim()).filter(Boolean));
  let next = [...models];
  for (const id of enabled) {
    next = ensureProfileModel(next, id);
  }
  return next.map((model) => ({
    ...model,
    enabled: enabled.has(model.id.trim()),
  }));
}

function MiniCapabilityToggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { t } = useI18n();
  return (
    <label
      className={`inline-flex h-7 items-center gap-1 rounded-md border px-1.5 text-[10px] ${
        checked
          ? "border-primary bg-primary/10"
          : "border-border/70 hover:bg-accent/30"
      }`}
    >
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="h-3 w-3 accent-primary"
      />
      <span>{t(label)}</span>
    </label>
  );
}

function mergedProfileModelCapabilities(
  endpoint: CatalogEntry["endpoints"][number],
  model: ProfileModelConfig,
) {
  return {
    image_input:
      !!endpoint.capabilities?.content?.image_input ||
      !!model.capabilities?.image_input,
    file_input:
      !!endpoint.capabilities?.content?.file_input ||
      !!model.capabilities?.file_input,
    web_search:
      !!endpoint.capabilities?.content?.web_search ||
      !!model.capabilities?.web_search,
  };
}

function formatContextWindow(value: number): string {
  if (value >= 1_000_000) return `${Math.round(value / 1_000_000)}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
  return String(value);
}

function ModelCatalogDialog({
  provider,
  apiType,
  endpoint,
  baseUrl,
  models,
  selectedModel,
  checkedModels,
  statuses,
  statusKeyFor,
  testingDisabled,
  onDefaultModelChange,
  onCheckedModelsChange,
  onModelsChange,
  onTestSelected,
  onClose,
}: {
  provider: CatalogEntry;
  apiType: string;
  endpoint: CatalogEntry["endpoints"][number];
  baseUrl: string;
  models: ProfileModelConfig[];
  selectedModel: string;
  checkedModels: string[];
  statuses: Record<string, ModelTestStatus>;
  statusKeyFor: (model: string) => string;
  testingDisabled: boolean;
  onDefaultModelChange: (model: string) => void;
  onCheckedModelsChange: (models: string[]) => void;
  onModelsChange: (models: ProfileModelConfig[], preferredDefaultModel?: string) => void;
  onTestSelected: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [customModel, setCustomModel] = useState("");
  const modelRows = models;
  const allModelIds = modelRows.map((model) => model.id);
  // Every model on this endpoint is tested against the same URL, so any row
  // that passed answers "where did the request actually go?" for the dialog.
  const testedUrl = modelRows.reduce<string | undefined>((found, model) => {
    const status = statuses[statusKeyFor(model.id)];
    return status?.state === "success" && status.url ? status.url : found;
  }, undefined);
  const allChecked =
    allModelIds.length > 0 &&
    allModelIds.every((model) => checkedModels.includes(model));
  const anyTesting = checkedModels.some(
    (model) => statuses[statusKeyFor(model)]?.state === "testing",
  );

  function setChecked(model: string, checked: boolean) {
    onCheckedModelsChange(
      checked
        ? Array.from(new Set([...checkedModels, model]))
        : checkedModels.filter((candidate) => candidate !== model),
    );
  }

  function updateModel(modelId: string, patch: Partial<ProfileModelConfig>) {
    onModelsChange(
      modelRows.map((model) =>
        model.id === modelId ? { ...model, ...patch } : model,
      ),
    );
  }

  function addCustomModel() {
    const id = customModel.trim();
    if (!id) return;
    const nextModels = ensureProfileModel(modelRows, id);
    onModelsChange(nextModels, selectedModel || id);
    setCustomModel("");
  }

  function removeCustomModel(modelId: string) {
    const nextModels = modelRows.filter((model) => model.id !== modelId);
    const nextDefault =
      selectedModel === modelId ? enabledProfileModelIds(nextModels)[0] : selectedModel;
    onModelsChange(nextModels, nextDefault);
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(820px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(820px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 border-b border-border px-5 py-3 pr-12">
          <DialogTitle className="flex min-w-0 flex-wrap items-center gap-2 text-base">
            <span className="truncate">{provider.label}</span>
            <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] font-normal text-muted-foreground">
              {apiTypeShort(apiType)}
            </span>
            <span className="text-xs font-normal text-muted-foreground">
              · {t(apiTypeLabel(apiType))}
            </span>
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t("Provider model catalog and connection tests.")}
          </DialogDescription>
          <div className="min-w-0 truncate pt-1 font-mono text-xs text-muted-foreground">
            {baseUrl || t("Endpoint URL required")}
          </div>
          {testedUrl && (
            <div className="flex min-w-0 items-baseline gap-1.5 pt-0.5 text-[11px]">
              <span className="shrink-0 text-muted-foreground">
                {t("Tested")}
              </span>
              <span
                className="min-w-0 truncate font-mono text-primary"
                title={testedUrl}
              >
                {testedUrl}
              </span>
            </div>
          )}
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-auto px-5 py-3 [scrollbar-gutter:stable]">
          <div className="mb-3 flex min-w-0 gap-2">
            <Input
              type="text"
              value={customModel}
              onChange={(event) => setCustomModel(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  addCustomModel();
                }
              }}
              placeholder={t("custom model")}
              className={MONO_INPUT_CLASS}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 shrink-0 gap-1.5"
              onClick={addCustomModel}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("Add")}
            </Button>
          </div>
          <div className="overflow-hidden rounded-md border border-border/70">
            <div className="grid grid-cols-[44px_44px_minmax(180px,1fr)_76px_minmax(150px,1fr)_minmax(84px,120px)] items-center gap-2 border-b border-border/70 bg-muted/50 px-2 py-1.5 text-[10px] font-medium text-muted-foreground">
              <span>{t("Default")}</span>
              <input
                type="checkbox"
                checked={allChecked}
                onChange={(event) =>
                  onCheckedModelsChange(event.target.checked ? allModelIds : [])
                }
                className="h-3.5 w-3.5 accent-primary"
                aria-label={t("Enable all models")}
              />
              <span>{t("Model")}</span>
              <span>{t("Context")}</span>
              <span>{t("Capabilities")}</span>
              <span>{t("Test")}</span>
            </div>
            {modelRows.map((model) => {
              const checked = checkedModels.includes(model.id);
              const status = statuses[statusKeyFor(model.id)] ?? { state: "idle" };
              const capabilities = mergedProfileModelCapabilities(endpoint, model);
              return (
                <div
                  key={model.id}
                  className={`grid min-h-10 grid-cols-[44px_44px_minmax(180px,1fr)_76px_minmax(150px,1fr)_minmax(84px,120px)] items-center gap-2 border-b border-border/50 px-2 py-1.5 text-xs last:border-b-0 ${
                    checked ? "bg-primary/5" : "hover:bg-accent/30"
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => onDefaultModelChange(model.id)}
                    className={`inline-flex h-7 w-7 items-center justify-center rounded-md ${
                      selectedModel === model.id
                        ? "text-primary"
                        : "text-muted-foreground/50 hover:bg-accent/40 hover:text-foreground"
                    }`}
                    title={t("Set default model")}
                    aria-label={t("Set default model")}
                  >
                    <Star
                      className="h-3.5 w-3.5"
                      fill={selectedModel === model.id ? "currentColor" : "none"}
                    />
                  </button>
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={(event) => setChecked(model.id, event.target.checked)}
                    className="h-3.5 w-3.5 accent-primary"
                    aria-label={t("Enable model")}
                  />
                  <div className="flex min-w-0 items-center gap-1.5">
                    <span
                      className={`min-w-0 truncate ${
                        model.label ? "text-[12px]" : "font-mono text-[12px]"
                      }`}
                      title={model.id}
                    >
                      {model.label || model.id}
                    </span>
                    {model.custom && (
                      <button
                        type="button"
                        onClick={() => removeCustomModel(model.id)}
                        className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/40 hover:text-destructive"
                        title={t("Delete")}
                        aria-label={t("Delete")}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    )}
                  </div>
                  <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground">
                    {model.context_window ? formatContextWindow(model.context_window) : "?"}
                  </span>
                  {model.custom ? (
                    <div className="flex min-w-0 items-center gap-1">
                      <MiniCapabilityToggle
                        label="Img"
                        checked={!!model.capabilities?.image_input}
                        onChange={(checked) =>
                          updateModel(model.id, {
                            capabilities: {
                              ...(model.capabilities ?? {}),
                              image_input: checked,
                            },
                          })
                        }
                      />
                      <MiniCapabilityToggle
                        label="File"
                        checked={!!model.capabilities?.file_input}
                        onChange={(checked) =>
                          updateModel(model.id, {
                            capabilities: {
                              ...(model.capabilities ?? {}),
                              file_input: checked,
                            },
                          })
                        }
                      />
                      <MiniCapabilityToggle
                        label="Web"
                        checked={!!model.capabilities?.web_search}
                        onChange={(checked) =>
                          updateModel(model.id, {
                            capabilities: {
                              ...(model.capabilities ?? {}),
                              web_search: checked,
                            },
                          })
                        }
                      />
                    </div>
                  ) : (
                    <span className="min-w-0 truncate text-[11px] text-muted-foreground">
                      {capabilityText(capabilities, t)}
                    </span>
                  )}
                  <ModelTestStatusBadge status={status} />
                </div>
              );
            })}
            {modelRows.length === 0 && (
              <div className="px-2 py-6 text-center text-xs text-muted-foreground">
                {t("No models")}
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="shrink-0 border-t border-border px-5 py-3 sm:justify-between">
          <div className="flex min-w-0 items-center text-xs text-muted-foreground">
            {t("{{count}} enabled", { count: checkedModels.length })}
          </div>
          <div className="flex items-center gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={onClose}>
              {t("Close")}
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={onTestSelected}
              disabled={testingDisabled || anyTesting || checkedModels.length === 0}
            >
              {anyTesting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <FlaskConical className="h-3.5 w-3.5" />
              )}
              {anyTesting ? t("Testing…") : t("Test selected")}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function capabilityText(
  capabilities: ReturnType<typeof mergedProfileModelCapabilities>,
  t: (key: string, vars?: Record<string, string | number | null | undefined>) => string,
) {
  const items = [
    capabilities.image_input ? t("images") : null,
    capabilities.file_input ? t("files") : null,
    capabilities.web_search ? t("web search") : null,
  ].filter((item): item is string => !!item);
  return items.length > 0 ? items.join(", ") : t("text");
}

function ModelTestStatusBadge({ status }: { status: ModelTestStatus }) {
  const { t } = useI18n();
  if (status.state === "testing") {
    return (
      <span className="inline-flex min-w-0 items-center gap-1 text-[11px] text-muted-foreground">
        <Loader2 className="h-3 w-3 shrink-0 animate-spin" />
        <span className="truncate">{t("Testing…")}</span>
      </span>
    );
  }
  if (status.state === "success") {
    return (
      <span className="inline-flex min-w-0 items-center gap-1 text-[11px] text-primary">
        <CheckCircle2 className="h-3 w-3 shrink-0" />
        <span className="truncate">{status.message}</span>
      </span>
    );
  }
  if (status.state === "error") {
    return (
      <span
        className="inline-flex min-w-0 items-center gap-1 text-[11px] text-destructive"
        title={status.message}
      >
        <AlertCircle className="h-3 w-3 shrink-0" />
        <span className="truncate">{t("Failed")}</span>
      </span>
    );
  }
  return <span className="text-[11px] text-muted-foreground/50">-</span>;
}

function GoogleOAuthField({
  status,
  loading,
  error,
  onLogin,
}: {
  status: GoogleOAuthStatus | null;
  loading: boolean;
  error: string | null;
  onLogin: () => void;
}) {
  const { t } = useI18n();
  const signedIn = !!status?.signedIn;

  return (
    <div>
      <div className="text-[11px] font-medium text-muted-foreground mb-0.5">
        {t("Google account")}
      </div>
      <div
        className={`flex min-h-10 items-center justify-between gap-3 rounded-md border px-2.5 py-2 text-xs ${
          signedIn ? "border-primary bg-primary/10" : "border-border"
        }`}
      >
        <div className="min-w-0 flex items-center gap-2">
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary" />
          ) : signedIn ? (
            <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-primary" />
          ) : (
            <LogIn className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          )}
          <span className="truncate font-medium">
            {signedIn ? t("Signed in with Google") : t("Google account not connected")}
          </span>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onLogin}
          disabled={loading}
          className="h-8"
        >
          {loading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <LogIn className="h-3.5 w-3.5" />
          )}
          {signedIn ? t("Reconnect") : t("Sign in")}
        </Button>
      </div>
      {error && (
        <div className="mt-1 text-[10px] text-destructive">{error}</div>
      )}
    </div>
  );
}

function ProxyField({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { t } = useI18n();

  return (
    <label
      className={`flex min-h-10 items-center justify-between gap-3 rounded-md border px-2.5 py-2 text-xs ${
        checked
          ? "border-primary bg-primary/10 cursor-pointer"
          : "border-border hover:bg-accent/30 cursor-pointer"
      }`}
    >
      <span className="min-w-0 font-medium">{t("Use HTTP proxy")}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-3.5 w-3.5 shrink-0 accent-primary"
      />
    </label>
  );
}

function DeepSeekBridgeSettingsField({
  settings,
  onChange,
}: {
  settings: ProviderSettings;
  onChange: (v: ProviderSettings) => void;
}) {
  const deepseek = settings.deepseek ?? {};
  const thinking = !!deepseek.thinking;

  function update(next: { thinking?: boolean; replay_reasoning_content?: boolean }) {
    const merged = {
      ...deepseek,
      ...next,
    };
    if (next.thinking === false) {
      merged.replay_reasoning_content = false;
    } else if (next.thinking === true && deepseek.replay_reasoning_content == null) {
      merged.replay_reasoning_content = true;
    }
    onChange({ ...settings, deepseek: merged });
  }

  return (
    <div className="space-y-2">
      <CheckRow
        label="Thinking mode"
        checked={thinking}
        onChange={(checked) => update({ thinking: checked })}
      />
      <CheckRow
        label="Replay reasoning content"
        checked={!!deepseek.replay_reasoning_content}
        disabled={!thinking}
        onChange={(checked) => update({ replay_reasoning_content: checked })}
      />
    </div>
  );
}

function CheckRow({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { t } = useI18n();

  return (
    <label
      className={`h-8 flex items-center gap-2 px-2.5 border rounded-md text-xs ${
        disabled
          ? "opacity-50 cursor-not-allowed border-border/70"
          : checked
            ? "border-primary bg-primary/10 cursor-pointer"
            : "border-border hover:bg-accent/30 cursor-pointer"
      }`}
    >
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
        className="h-3.5 w-3.5 accent-primary"
      />
      <span>{t(label)}</span>
    </label>
  );
}

function FormSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="space-y-3 border-t border-border/60 pt-3 first:border-t-0 first:pt-0">
      <div className="text-xs font-semibold">{title}</div>
      {children}
    </section>
  );
}

function ApiKindsField({
  endpoints,
  editable,
  selectedApiTypes,
  setSelectedApiTypes,
  onOpenModels,
}: {
  endpoints: CatalogEntry["endpoints"];
  editable: boolean;
  selectedApiTypes: string[];
  setSelectedApiTypes: (v: string[]) => void;
  onOpenModels?: (endpoint: CatalogEntry["endpoints"][number]) => void;
}) {
  const { t } = useI18n();

  return (
    <div>
      <div className="text-[11px] font-medium text-muted-foreground mb-1">
        {t("API kinds")}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {endpoints.map((ep) => {
          const checked = selectedApiTypes.includes(ep.api_type);
          const showModels = checked && !!onOpenModels;
          if (editable) {
            return (
              <label
                key={ep.api_type}
                className={`min-h-8 flex items-center gap-2 px-2.5 py-1 border rounded-md cursor-pointer text-xs ${
                  checked
                    ? "border-primary bg-primary/10"
                    : "border-border hover:bg-accent/30"
                }`}
              >
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={(e) => {
                    if (e.target.checked) {
                      setSelectedApiTypes([...selectedApiTypes, ep.api_type]);
                    } else {
                      setSelectedApiTypes(
                        selectedApiTypes.filter((a) => a !== ep.api_type),
                      );
                    }
                  }}
                  className="h-3.5 w-3.5 accent-primary"
                />
                <span className="font-mono">{apiTypeShort(ep.api_type)}</span>
                {showModels && (
                  <button
                    type="button"
                    onClick={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      onOpenModels(ep);
                    }}
                    className="ml-1 inline-flex h-6 items-center gap-1 rounded border border-primary/30 bg-background/70 px-1.5 text-[11px] text-primary hover:bg-background"
                  >
                    <ListChecks className="h-3 w-3" />
                    {t("Models")}
                  </button>
                )}
              </label>
            );
          }
          return (
            <div
              key={ep.api_type}
              className="min-h-8 flex items-center gap-2 px-2.5 py-1 border border-primary bg-primary/10 rounded-md text-xs"
            >
              <span className="font-mono">{apiTypeShort(ep.api_type)}</span>
              {showModels && (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="ml-1 h-6 px-1.5 text-[11px]"
                  onClick={() => onOpenModels(ep)}
                >
                  <ListChecks className="h-3 w-3" />
                  {t("Models")}
                </Button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function CredentialField({
  field,
  value,
  reveal,
  onChange,
  onToggleReveal,
}: {
  field: FieldDef;
  value: string;
  reveal: boolean;
  onChange: (v: string) => void;
  onToggleReveal: () => void;
}) {
  const { t } = useI18n();

  return (
    <FieldRow label={t(field.label)} required={field.required}>
      <div className="relative">
        <Input
          type={field.secret && !reveal ? "password" : "text"}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={field.placeholder ?? undefined}
          className={SECRET_INPUT_CLASS}
        />
        {field.secret && (
          <button
            type="button"
            onClick={onToggleReveal}
            className="absolute right-1 top-1/2 -translate-y-1/2 p-1 text-muted-foreground hover:text-foreground"
            aria-label={reveal ? t("Hide") : t("Reveal")}
          >
            {reveal ? (
              <EyeOff className="w-3 h-3" />
            ) : (
              <Eye className="w-3 h-3" />
            )}
          </button>
        )}
      </div>
    </FieldRow>
  );
}

function FieldRow({
  label,
  required,
  hint,
  children,
}: {
  label: string;
  required?: boolean;
  hint?: string;
  children: ReactNode;
}) {
  const { t } = useI18n();

  return (
    <label className="block">
      <div className="text-[11px] font-medium text-muted-foreground mb-0.5">
        {t(label)}
        {required && <span className="text-destructive ml-0.5">*</span>}
      </div>
      {children}
      {hint && (
        <div className="mt-0.5 text-[11px] leading-4 text-muted-foreground">
          {hint}
        </div>
      )}
    </label>
  );
}
