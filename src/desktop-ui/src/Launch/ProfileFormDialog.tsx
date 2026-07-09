/**
 * Two-step modal: pick a provider, then fill its credentials.
 *
 * Step 1 lets the user click any catalog tile. Step 2 builds a form by
 * intersecting the catalog API kinds' `fields[]`. We default to the
 * api_key auth mode and let custom providers multi-select API kinds when
 * one key supports more than one.
 */
import { useEffect, useMemo, useState } from "react";
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
import { CUSTOM_PROVIDER } from "./ProfileFormDialog.constants";
import { testProfileConnection } from "./api";
import { FormBody } from "./ProfileFormBody";
import { ProviderGrid } from "./ProfileProviderGrid";
import {
  arraysEqual,
  collectFields,
  defaultAuthMode,
  defaultApiKindEndpoints,
  overridesForEndpoints,
  pruneOverrides,
  pruneProviderSettings,
  providerApiKindEndpoints,
  providerApiKindsEditable,
  providerUsesEndpointGroups,
  requiresProfileModel,
  selectedEndpointGroup,
  selectedEndpoint,
  stripEmpty,
  syncApiConfigsForProvider,
} from "./profileFormHelpers";
import type {
  ApiTypeOverrides,
  AuthMode,
  CatalogEntry,
  ProfileApiConfig,
  ProfileDef,
  ProfileDraft,
  ProviderSettings,
} from "./types";
import { isProviderApiKind } from "./types";

type Step = "pick-provider" | "fill-form";
export type ProfileFormSubmit =
  | { type: "create"; draft: ProfileDraft }
  | { type: "update"; profile: ProfileDef };

interface Props {
  catalog: CatalogEntry[];
  /** Set when editing -- locks step 1 and prefills step 2. */
  initial?: ProfileDef | null;
  onClose: () => void;
  onSave: (submit: ProfileFormSubmit) => Promise<void>;
}

export function ProfileFormDialog({
  catalog,
  initial,
  onClose,
  onSave,
}: Props) {
  const { t } = useI18n();
  const editing = !!initial;

  const { initialProvider, providerMissing } = useMemo(() => {
    if (!initial) return { initialProvider: null, providerMissing: false };
    if (initial.provider === "custom") {
      return { initialProvider: CUSTOM_PROVIDER, providerMissing: false };
    }
    const found = catalog.find((c) => c.id === initial.provider);
    if (!found) {
      return { initialProvider: CUSTOM_PROVIDER, providerMissing: true };
    }
    return { initialProvider: found, providerMissing: false };
  }, [catalog, initial]);

  const [step, setStep] = useState<Step>(
    editing ? "fill-form" : "pick-provider",
  );
  const [provider, setProvider] = useState<CatalogEntry | null>(
    initialProvider,
  );
  const [label, setLabel] = useState(initial?.label ?? "");
  const [selectedApiTypes, setSelectedApiTypes] = useState<string[]>(
    initialSelectedApiTypes(initial),
  );
  const [authMode, setAuthMode] = useState<AuthMode>(
    initial?.auth_mode ?? "api_key",
  );
  const [credentials, setCredentials] = useState<Record<string, string>>(
    initial?.credentials ?? {},
  );
  const [overrides, setOverrides] = useState<Record<string, ApiTypeOverrides>>(
    initial?.overrides ?? {},
  );
  const [apiConfigs, setApiConfigs] = useState<Record<string, ProfileApiConfig>>(
    initial?.api_configs ?? {},
  );
  const [useSettingsProxy, setUseSettingsProxy] = useState(
    !!initial?.use_settings_proxy,
  );
  const [providerSettings, setProviderSettings] = useState<ProviderSettings>(
    initial?.provider_settings ?? {},
  );
  const [revealKeys, setRevealKeys] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!provider || editing) return;
    if (provider.id === "custom") {
      setSelectedApiTypes([]);
      setOverrides({});
      setApiConfigs(syncApiConfigsForProvider(provider, [], {}, {}));
      setAuthMode(defaultAuthMode(provider, [], {}));
      setProviderSettings({});
      return;
    }
    const apiKindEndpoints = defaultApiKindEndpoints(provider);
    const apiTypes = apiKindEndpoints.map((e) => e.api_type);
    const nextOverrides = overridesForEndpoints(apiKindEndpoints);
    setSelectedApiTypes(apiTypes);
    setOverrides(nextOverrides);
    setApiConfigs(syncApiConfigsForProvider(provider, apiTypes, nextOverrides, {}));
    setAuthMode(defaultAuthMode(provider, apiTypes, nextOverrides));
    setProviderSettings(
      provider.id === "deepseek"
        ? {
            deepseek: {
              thinking: true,
              replay_reasoning_content: true,
            },
          }
        : {},
    );
  }, [provider, editing]);

  useEffect(() => {
    if (!provider || providerApiKindsEditable(provider)) return;
    const endpoints = providerUsesEndpointGroups(provider)
      ? (selectedEndpointGroup(provider, selectedApiTypes, overrides)?.endpoints ??
        defaultApiKindEndpoints(provider))
      : providerApiKindEndpoints(provider);
    const apiKinds = endpoints.map((e) => e.api_type);
    setSelectedApiTypes((current) =>
      arraysEqual(current, apiKinds) ? current : apiKinds,
    );
    setApiConfigs((current) =>
      syncApiConfigsForProvider(provider, apiKinds, overrides, current),
    );
    setAuthMode((current) =>
      defaultAuthMode(provider, apiKinds, overrides, current),
    );
  }, [provider, overrides, selectedApiTypes]);

  useEffect(() => {
    if (!provider || selectedApiTypes.length === 0) return;
    setAuthMode((current) =>
      defaultAuthMode(provider, selectedApiTypes, overrides, current),
    );
  }, [provider, overrides, selectedApiTypes]);

  function handlePickProvider(c: CatalogEntry) {
    setProvider(c);
    if (!label) setLabel(c.label);
    setStep("fill-form");
  }

  function buildDraftForProvider(
    selectedProvider: CatalogEntry,
    formLabel: string,
  ): ProfileDraft {
    const fieldDefs = collectFields(selectedProvider, selectedApiTypes, authMode, overrides);
    const credentialFieldNames = new Set(fieldDefs.map((field) => field.name));
    const selectedCredentials = Object.fromEntries(
      Object.entries(credentials).filter(([name]) =>
        credentialFieldNames.has(name),
      ),
    );

    const nextApiConfigs = syncApiConfigsForProvider(
      selectedProvider,
      selectedApiTypes,
      overrides,
      apiConfigs,
    );

    return {
      label: formLabel,
      provider: selectedProvider.id,
      auth_mode: authMode,
      api_types: selectedApiTypes,
      credentials: stripEmpty(selectedCredentials),
      overrides: pruneOverrides(overrides, selectedApiTypes, selectedProvider),
      api_configs: nextApiConfigs,
      use_settings_proxy: useSettingsProxy,
      provider_settings: pruneProviderSettings(selectedProvider.id, providerSettings),
    };
  }

  function selectedProfileModel(apiType: string): string {
    const overrideModel = overrides[apiType]?.model?.trim();
    if (overrideModel) return overrideModel;
    const config = apiConfigs[apiType];
    const configModel = config?.model?.trim();
    if (configModel) return configModel;
    return (
      config?.models
        ?.find((model) => model.enabled !== false)
        ?.id.trim() ?? ""
    );
  }

  function validateProfileDraft({
    requireLabel,
    requireApiKeyAuth,
  }: {
    requireLabel: boolean;
    requireApiKeyAuth?: boolean;
  }): { draft: ProfileDraft } | { error: string } {
    if (!provider) return { error: t("Pick a provider") };
    const formLabel = label.trim();
    if (requireLabel && !formLabel) {
      return { error: t("Label is required") };
    }
    if (selectedApiTypes.length === 0) {
      return { error: t("Pick at least one API type") };
    }
    if (requireApiKeyAuth && authMode !== "api_key") {
      return { error: t("Connection test currently supports API key profiles.") };
    }

    const fieldDefs = collectFields(provider, selectedApiTypes, authMode, overrides);
    for (const f of fieldDefs) {
      if (f.required && !credentials[f.name]?.trim()) {
        return { error: t("{{field}} is required", { field: t(f.label) }) };
      }
    }

    for (const apiType of selectedApiTypes) {
      const ep = selectedEndpoint(provider, apiType, overrides);
      if (!ep) continue;
      const ov = overrides[apiType];
      if (requiresProfileModel(provider, ep) && !selectedProfileModel(apiType)) {
        return { error: t("Model is required for {{apiType}}", { apiType }) };
      }
      if (ep.default_base_url) continue;
      if (!ov?.base_url?.trim()) {
        return { error: t("Base URL is required for {{apiType}}", { apiType }) };
      }
    }

    return { draft: buildDraftForProvider(provider, formLabel || provider.label) };
  }

  async function handleSave() {
    setError(null);
    const result = validateProfileDraft({ requireLabel: true });
    if ("error" in result) {
      setError(result.error);
      return;
    }

    setSaving(true);
    try {
      await onSave(
        initial
          ? {
              type: "update",
              profile: {
                id: initial.id,
                ...result.draft,
                connections: initial.connections,
              },
            }
          : { type: "create", draft: result.draft },
      );
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSaving(false);
    }
  }

  async function handleTestModel(apiType: string, model: string) {
    setError(null);
    const result = validateProfileDraft({
      requireLabel: false,
      requireApiKeyAuth: true,
    });
    if ("error" in result) {
      return { ok: false, message: result.error };
    }

    const testOverrides = {
      ...result.draft.overrides,
      [apiType]: {
        ...(result.draft.overrides[apiType] ?? {}),
        model,
      },
    };
    const testDraft: ProfileDraft = {
      ...result.draft,
      api_types: [apiType],
      overrides: testOverrides,
      api_configs: provider
        ? syncApiConfigsForProvider(
            provider,
            [apiType],
            testOverrides,
            result.draft.api_configs ?? {},
          )
        : result.draft.api_configs,
    };

    try {
      await testProfileConnection(testDraft);
      return { ok: true, message: t("Test passed") };
    } catch (e) {
      return {
        ok: false,
        message: e instanceof Error ? e.message : String(e),
      };
    }
  }

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="!flex max-h-[calc(100vh-64px)] w-[min(960px,calc(100vw-32px))] max-w-[calc(100vw-32px)] flex-col overflow-hidden p-0 sm:max-w-[min(960px,calc(100vw-32px))]">
        <DialogHeader className="shrink-0 border-b border-border px-6 py-4 pr-12">
          <DialogTitle>
            {editing
              ? t("Edit profile · {{label}}", { label: initial!.label })
              : step === "pick-provider"
                ? t("Pick a provider")
                : t("New profile · {{provider}}", { provider: provider?.label ?? "" })}
          </DialogTitle>
          <DialogDescription className="sr-only">
            {t("Configure a Quick Launch provider profile.")}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4 [scrollbar-gutter:stable]">
          {step === "pick-provider" ? (
            <ProviderGrid catalog={catalog} onPick={handlePickProvider} />
          ) : provider ? (
            <FormBody
              provider={provider}
              label={label}
              setLabel={setLabel}
              selectedApiTypes={selectedApiTypes}
              setSelectedApiTypes={setSelectedApiTypes}
              authMode={authMode}
              setAuthMode={setAuthMode}
              credentials={credentials}
              setCredentials={setCredentials}
              overrides={overrides}
              setOverrides={setOverrides}
              apiConfigs={apiConfigs}
              setApiConfigs={setApiConfigs}
              useSettingsProxy={useSettingsProxy}
              setUseSettingsProxy={setUseSettingsProxy}
              providerSettings={providerSettings}
              setProviderSettings={setProviderSettings}
              revealKeys={revealKeys}
              setRevealKeys={setRevealKeys}
              testingDisabled={saving}
              onTestModel={handleTestModel}
            />
          ) : null}
        </div>

        {providerMissing && (
          <div className="shrink-0 border-t border-amber-500/20 bg-amber-500/10 px-6 py-2 text-xs text-amber-700">
            ⚠ {t("The provider {{provider}} is no longer in the catalog. Form fell back to a custom endpoint — re-pick a provider via Back, or edit the URL/key and save.", {
              provider: initial?.provider ?? "",
            })}
          </div>
        )}
        {error && (
          <div className="shrink-0 border-t border-destructive/20 bg-destructive/10 px-6 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        <DialogFooter className="shrink-0 border-t border-border px-6 py-4 sm:justify-between">
          <div className="flex min-w-0 flex-wrap items-center gap-3">
            {step === "fill-form" && !editing && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => setStep("pick-provider")}
              >
                {t("Change provider")}
              </Button>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={onClose}>
              {t("Cancel")}
            </Button>
            {step === "fill-form" && (
              <Button
                type="button"
                size="sm"
                onClick={handleSave}
                disabled={saving}
              >
                {saving
                  ? t("Saving…")
                  : editing
                    ? t("Save changes")
                    : t("Create profile")}
              </Button>
            )}
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function initialSelectedApiTypes(initial: ProfileDef | null | undefined): string[] {
  if (!initial) return [];
  const configs = initial.api_configs ?? {};
  const configEntries = Object.entries(configs);
  if (configEntries.length > 0) {
    return configEntries
      .filter(([apiType, config]) => isProviderApiKind(apiType) && !!config.enabled)
      .map(([apiType]) => apiType);
  }
  return Array.from(new Set((initial.api_types ?? []).filter(isProviderApiKind)));
}
