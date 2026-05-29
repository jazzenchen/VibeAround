/**
 * Launch tab — agent-first builder for choosing connection, workspace, and
 * session before opening a real coding-agent CLI in the user's Terminal.
 */
import { useCallback, useEffect, useState } from "react";
import { useI18n } from "@va/i18n";

import {
  createProfile,
  getLauncherPreferences,
  getProfile,
  listCatalog,
  listProfiles,
  setProfileConnection,
  upsertProfile,
  type LauncherPreferences,
} from "./api";
import { AgentLaunchBuilder } from "./AgentLaunchBuilder";
import { ProfileConnectionDialog } from "./ProfileConnectionDialog";
import { ProfileFormDialog } from "./ProfileFormDialog";
import type { ProfileFormSubmit } from "./ProfileFormDialog";
import type {
  CatalogEntry,
  ConnectionAgentId,
  ProfileConnectionPreference,
  ProfileDef,
  ProfileSummary,
} from "./types";

type ConnectionEditing = {
  profile: ProfileSummary;
  agentId: ConnectionAgentId;
};

export function Launch({ refreshToken = 0 }: { refreshToken?: number }) {
  const { t } = useI18n();
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [prefs, setPrefs] = useState<LauncherPreferences | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editing, setEditing] = useState<ProfileDef | null>(null);
  const [connectionEditing, setConnectionEditing] =
    useState<ConnectionEditing | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [nextCatalog, nextProfiles, nextPrefs] = await Promise.all([
        listCatalog(),
        listProfiles(),
        getLauncherPreferences(),
      ]);
      setCatalog(nextCatalog);
      setProfiles(nextProfiles);
      setPrefs(nextPrefs);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh, refreshToken]);

  useEffect(() => {
    if (!toast) return;
    const timeout = setTimeout(() => setToast(null), 2500);
    return () => clearTimeout(timeout);
  }, [toast]);

  function openNewEditor() {
    setEditing(null);
    setEditorOpen(true);
  }

  async function handleEdit(summary: ProfileSummary) {
    try {
      const full = await getProfile(summary.id);
      setEditing(full);
      setEditorOpen(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleSave(submit: ProfileFormSubmit) {
    if (submit.type === "create") {
      await createProfile(submit.draft);
    } else {
      await upsertProfile(submit.profile);
    }
    await refresh();
  }

  async function handleSaveConnection(
    agentId: ConnectionAgentId,
    preference: ProfileConnectionPreference,
  ) {
    if (!connectionEditing) return;
    await setProfileConnection(
      connectionEditing.profile.id,
      agentId,
      preference,
    );
    const nextPrefs = await getLauncherPreferences();
    setPrefs(nextPrefs);
  }

  return (
    <div className="relative flex h-full flex-col">
      {(error || toast) && (
        <div className="pointer-events-none absolute inset-x-3 top-3 z-40 flex flex-col items-center gap-2">
          {error && (
            <div
              role="alert"
              className="max-w-[min(720px,100%)] rounded-md border border-destructive/25 bg-background/95 px-3 py-2 text-xs text-destructive shadow-lg shadow-destructive/10 backdrop-blur"
            >
              {error}
            </div>
          )}
          {toast && (
            <div
              role="status"
              aria-live="polite"
              className="max-w-[min(520px,100%)] rounded-md border border-emerald-500/20 bg-background/95 px-3 py-2 text-xs text-emerald-600 shadow-lg shadow-emerald-500/10 backdrop-blur"
            >
              {toast}
            </div>
          )}
        </div>
      )}

      {loading ? (
        <p className="p-3 text-xs text-muted-foreground">{t("Loading…")}</p>
      ) : (
        <AgentLaunchBuilder
          profiles={profiles}
          prefs={prefs}
          onPrefsChange={setPrefs}
          onProfilesChange={setProfiles}
          onNewProfile={openNewEditor}
          onEditProfile={(profile) => void handleEdit(profile)}
          onConnectionSettings={(profile, agentId) =>
            setConnectionEditing({ profile, agentId })
          }
          onError={setError}
          onToast={setToast}
        />
      )}

      {editorOpen && (
        <ProfileFormDialog
          catalog={catalog}
          initial={editing}
          onClose={() => {
            setEditorOpen(false);
            setEditing(null);
          }}
          onSave={handleSave}
        />
      )}
      {connectionEditing && (
        <ProfileConnectionDialog
          profile={connectionEditing.profile}
          agentId={connectionEditing.agentId}
          connections={prefs?.profileConnections}
          onClose={() => setConnectionEditing(null)}
          onSave={handleSaveConnection}
        />
      )}
    </div>
  );
}
