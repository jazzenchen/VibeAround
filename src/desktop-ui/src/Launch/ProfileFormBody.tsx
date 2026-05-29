import { useEffect, type ReactNode } from "react";

import { Eye, EyeOff, Globe } from "lucide-react";
import { useI18n } from "@va/i18n";

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
  apiKindHint,
  arraysEqual,
  canOverrideInputSupport,
  collectFields,
  endpointId,
  endpointLabel,
  endpointsForApiType,
  overridesForEndpoints,
  providerEndpointGroups,
  providerApiKindsEditable,
  providerApiKindEndpoints,
  providerUsesEndpointGroups,
  selectedEndpointGroup,
  selectedEndpoint,
  shouldShowBaseUrl,
} from "./profileFormHelpers";
import type { ApiTypeOverrides, CatalogEntry, FieldDef } from "./types";
import type { ProviderSettings } from "./types";
import { apiTypeLabel, apiTypeShort } from "./types";

interface FormBodyProps {
  provider: CatalogEntry;
  label: string;
  setLabel: (v: string) => void;
  selectedApiTypes: string[];
  setSelectedApiTypes: (v: string[]) => void;
  credentials: Record<string, string>;
  setCredentials: (v: Record<string, string>) => void;
  overrides: Record<string, ApiTypeOverrides>;
  setOverrides: (v: Record<string, ApiTypeOverrides>) => void;
  useSettingsProxy: boolean;
  setUseSettingsProxy: (v: boolean) => void;
  providerSettings: ProviderSettings;
  setProviderSettings: (v: ProviderSettings) => void;
  revealKeys: Record<string, boolean>;
  setRevealKeys: (v: Record<string, boolean>) => void;
}

export function FormBody({
  provider,
  label,
  setLabel,
  selectedApiTypes,
  setSelectedApiTypes,
  credentials,
  setCredentials,
  overrides,
  setOverrides,
  useSettingsProxy,
  setUseSettingsProxy,
  providerSettings,
  setProviderSettings,
  revealKeys,
  setRevealKeys,
}: FormBodyProps) {
  const { t } = useI18n();
  const endpointGroups = providerEndpointGroups(provider);
  const usesEndpointGroups = providerUsesEndpointGroups(provider);
  const selectedGroup = usesEndpointGroups
    ? selectedEndpointGroup(provider, selectedApiTypes, overrides)
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
    "api_key",
    overrides,
  );
  const apiKindsEditable = providerApiKindsEditable(provider);

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

  function applyEndpointGroup(groupId: string) {
    const group = endpointGroups.find((candidate) => candidate.id === groupId);
    if (!group) return;
    setSelectedApiTypes(group.endpoints.map((endpoint) => endpoint.api_type));
    setOverrides(overridesForEndpoints(group.endpoints, overrides));
  }

  function applySelectedApiTypes(apiTypes: string[]) {
    const nextOverrides = { ...overrides };
    for (const endpoint of visibleApiKindEndpoints) {
      if (!apiTypes.includes(endpoint.api_type)) continue;
      if (nextOverrides[endpoint.api_type]) continue;
      nextOverrides[endpoint.api_type] = overridesForEndpoints(
        [endpoint],
        nextOverrides,
      )[endpoint.api_type];
    }
    setOverrides(nextOverrides);
    setSelectedApiTypes(apiTypes);
  }

  return (
    <div className="space-y-3">
      <FormSection title={t("Profile")}>
        <FieldRow label={t("Label")} hint={t("Visible name for this profile.")}>
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
        />

        <ProxyField
          checked={useSettingsProxy}
          onChange={setUseSettingsProxy}
        />
      </FormSection>

      {fieldDefs.length > 0 && (
        <FormSection title={t("Credentials")}>
          {fieldDefs.map((f) => (
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
      )}

      {effectiveSelectedApiTypes.length > 0 && (
        <FormSection title={t("Model settings")}>
          <div className="space-y-2">
            {effectiveSelectedApiTypes.map((apiType) => {
              const ep = selectedEndpoint(provider, apiType, overrides);
              if (!ep) return null;
              const ov = overrides[apiType] ?? {};
              const endpointOptions = endpointsForApiType(provider, apiType);
              const modelHint = apiKindHint(provider, apiType, ep);
              return (
                <div
                  key={apiType}
                  className="border border-border/60 rounded-md p-2.5 space-y-2"
                >
                  <div className="flex items-center gap-2 text-xs">
                    <span className="font-mono px-1.5 py-0.5 rounded bg-muted">
                      {apiTypeShort(apiType)}
                    </span>
                    <span className="text-muted-foreground/70">
                      · {t(apiTypeLabel(apiType))}
                    </span>
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
                          const modelStillValid = nextEndpoint.models.some(
                            (model) => model.id === ov.model,
                          );
                          setOverrides({
                            ...overrides,
                            [apiType]: {
                              ...ov,
                              endpoint_id: endpointId(nextEndpoint),
                              base_url: nextEndpoint.default_base_url || undefined,
                              model: modelStillValid
                                ? ov.model
                                : (nextEndpoint.models[0]?.id ?? ov.model ?? ""),
                            },
                          });
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
                              {endpointLabel(endpoint)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </FieldRow>
                  )}
                  {shouldShowBaseUrl(provider, ep, ov) && (
                    <FieldRow
                      label={
                        provider.id === "azure" || provider.id === "gemini"
                          ? "Endpoint"
                          : "Base URL"
                      }
                      required={ep.default_base_url === ""}
                      hint={
                        ep.default_base_url
                          ? t("Leave blank to use the catalog default.")
                          : provider.id === "custom"
                            ? t("Required for custom endpoints.")
                            : provider.id === "gemini"
                              ? t("Required for Vertex AI; use the endpoint root ending in /endpoints/openapi.")
                            : t("Endpoint URL from the provider dashboard.")
                      }
                    >
                      <Input
                        type="text"
                        value={ov.base_url ?? ""}
                        onChange={(e) =>
                          setOverrides({
                            ...overrides,
                            [apiType]: { ...ov, base_url: e.target.value },
                          })
                        }
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
                  <FieldRow
                    label={
                      provider.id === "azure" ? "Deployment name" : "Model"
                    }
                    hint={modelHint ? t(modelHint) : undefined}
                  >
                    {ep.models.length > 0 ? (
                      <Select
                        value={ov.model ?? ""}
                        onValueChange={(value) =>
                          setOverrides({
                            ...overrides,
                            [apiType]: { ...ov, model: value },
                          })
                        }
                      >
                        <SelectTrigger
                          size="sm"
                          className="h-8 w-full text-[13px]"
                        >
                          <SelectValue placeholder={t("Select a model")} />
                        </SelectTrigger>
                        <SelectContent>
                          {ep.models.map((m) => (
                            <SelectItem
                              key={m.id}
                              value={m.id}
                              className="text-xs"
                            >
                              {m.label ?? m.id}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <Input
                        type="text"
                        value={ov.model ?? ""}
                        onChange={(e) =>
                          setOverrides({
                            ...overrides,
                            [apiType]: { ...ov, model: e.target.value },
                          })
                        }
                        placeholder={t("model id (e.g. gpt-4o, claude-sonnet-4-6)")}
                        className={MONO_INPUT_CLASS}
                      />
                    )}
                  </FieldRow>
                  {ep.capabilities?.reasoning_effort && (
                    <FieldRow label={t("Reasoning effort")}>
                      <Select
                        value={ov.reasoning_effort ?? "medium"}
                        onValueChange={(value) =>
                          setOverrides({
                            ...overrides,
                            [apiType]: { ...ov, reasoning_effort: value },
                          })
                        }
                      >
                        <SelectTrigger
                          size="sm"
                          className="h-8 w-full text-[13px]"
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="low" className="text-xs">
                            low
                          </SelectItem>
                          <SelectItem value="medium" className="text-xs">
                            medium
                          </SelectItem>
                          <SelectItem value="high" className="text-xs">
                            high
                          </SelectItem>
                          <SelectItem value="xhigh" className="text-xs">
                            xhigh
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </FieldRow>
                  )}
                  {canOverrideInputSupport(provider, ep) && (
                    <FieldRow label={t("Input support")}>
                      <div className="grid grid-cols-2 gap-1.5">
                        <CheckRow
                          label="Images"
                          checked={!!ov.capabilities?.image_input}
                          onChange={(checked) =>
                            setOverrides({
                              ...overrides,
                              [apiType]: {
                                ...ov,
                                capabilities: {
                                  ...(ov.capabilities ?? {}),
                                  image_input: checked,
                                },
                              },
                            })
                          }
                        />
                        <CheckRow
                          label="Files"
                          checked={!!ov.capabilities?.file_input}
                          onChange={(checked) =>
                            setOverrides({
                              ...overrides,
                              [apiType]: {
                                ...ov,
                                capabilities: {
                                  ...(ov.capabilities ?? {}),
                                  file_input: checked,
                                },
                              },
                            })
                          }
                        />
                      </div>
                    </FieldRow>
                  )}
                </div>
              );
            })}
          </div>
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
    <FieldRow label={t("Plan type")}>
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
      <span className="min-w-0">
        <span className="block font-medium">{t("Use Settings proxy")}</span>
        <span className="block text-[10px] text-muted-foreground/70">
          {t("Provider requests for this profile use the configured Settings proxy when it is enabled.")}
        </span>
      </span>
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
}: {
  endpoints: CatalogEntry["endpoints"];
  editable: boolean;
  selectedApiTypes: string[];
  setSelectedApiTypes: (v: string[]) => void;
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
          if (editable) {
            return (
              <label
                key={ep.api_type}
                className={`h-8 flex items-center gap-2 px-2.5 border rounded-md cursor-pointer text-xs ${
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
                <span className="text-muted-foreground/70">
                  · {t(apiTypeLabel(ep.api_type))}
                </span>
              </label>
            );
          }
          return (
            <div
              key={ep.api_type}
              className="h-8 flex items-center gap-2 px-2.5 border border-primary bg-primary/10 rounded-md text-xs"
            >
              <span className="font-mono">{apiTypeShort(ep.api_type)}</span>
              <span className="text-muted-foreground/70">
                · {t(apiTypeLabel(ep.api_type))}
              </span>
            </div>
          );
        })}
      </div>
      {editable && (
        <p className="text-[10px] text-muted-foreground/60 mt-1">
          {t("Select every API shape this endpoint supports.")}
        </p>
      )}
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
  hint,
  required,
  children,
}: {
  label: string;
  hint?: string;
  required?: boolean;
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
        <div className="text-[10px] text-muted-foreground/60 mt-0.5">
          {t(hint)}
        </div>
      )}
    </label>
  );
}
