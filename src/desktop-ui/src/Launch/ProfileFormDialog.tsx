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
import { FormBody } from "./ProfileFormBody";
import { ProviderGrid } from "./ProfileProviderGrid";
import {
  arraysEqual,
  collectFields,
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
} from "./profileFormHelpers";
import type {
  ApiTypeOverrides,
  AuthMode,
  CatalogEntry,
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
    Array.from(new Set((initial?.api_types ?? []).filter(isProviderApiKind))),
  );
  const [credentials, setCredentials] = useState<Record<string, string>>(
    initial?.credentials ?? {},
  );
  const [overrides, setOverrides] = useState<Record<string, ApiTypeOverrides>>(
    initial?.overrides ?? {},
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
    const apiKindEndpoints = defaultApiKindEndpoints(provider);
    setSelectedApiTypes(apiKindEndpoints.map((e) => e.api_type));
    setOverrides(overridesForEndpoints(apiKindEndpoints));
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
  }, [provider, overrides, selectedApiTypes]);

  function handlePickProvider(c: CatalogEntry) {
    setProvider(c);
    if (!label) setLabel(c.label);
    setStep("fill-form");
  }

  async function handleSave() {
    setError(null);
    if (!provider) return;
    if (!label.trim()) {
      setError(t("Label is required"));
      return;
    }
    if (selectedApiTypes.length === 0) {
      setError(t("Pick at least one API type"));
      return;
    }

    const fieldDefs = collectFields(provider, selectedApiTypes, "api_key", overrides);
    for (const f of fieldDefs) {
      if (f.required && !credentials[f.name]?.trim()) {
        setError(t("{{field}} is required", { field: t(f.label) }));
        return;
      }
    }

    for (const apiType of selectedApiTypes) {
      const ep = selectedEndpoint(provider, apiType, overrides);
      if (!ep) continue;
      const ov = overrides[apiType];
      if (requiresProfileModel(provider, ep) && !ov?.model?.trim()) {
        setError(t("Model is required for {{apiType}}", { apiType }));
        return;
      }
      if (ep.default_base_url) continue;
      if (!ov?.base_url?.trim()) {
        setError(t("Base URL is required for {{apiType}}", { apiType }));
        return;
      }
    }

    const draft: ProfileDraft = {
      label: label.trim(),
      provider: provider.id,
      auth_mode: "api_key" as AuthMode,
      api_types: selectedApiTypes,
      credentials: stripEmpty(credentials),
      overrides: pruneOverrides(overrides, selectedApiTypes, provider),
      use_settings_proxy: useSettingsProxy,
      provider_settings: pruneProviderSettings(provider.id, providerSettings),
    };

    setSaving(true);
    try {
      await onSave(
        initial
          ? { type: "update", profile: { id: initial.id, ...draft } }
          : { type: "create", draft },
      );
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSaving(false);
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
              credentials={credentials}
              setCredentials={setCredentials}
              overrides={overrides}
              setOverrides={setOverrides}
              useSettingsProxy={useSettingsProxy}
              setUseSettingsProxy={setUseSettingsProxy}
              providerSettings={providerSettings}
              setProviderSettings={setProviderSettings}
              revealKeys={revealKeys}
              setRevealKeys={setRevealKeys}
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
          <div>
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
