import type { ProfileDef, ProfileDraft } from "./types";

export function copyProfileLabel(label: string, copySuffix: string): string {
  const trimmed = label.trim();
  const escapedSuffix = copySuffix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const existingCopyPattern = new RegExp(
    `^(.*)\\s+${escapedSuffix}(?:\\s+(\\d+))?$`,
  );
  const existing = trimmed.match(existingCopyPattern);
  if (!existing) return `${trimmed} ${copySuffix}`;

  const base = existing[1]?.trim() || trimmed;
  const nextNumber = Number(existing[2] ?? "1") + 1;
  return `${base} ${copySuffix} ${nextNumber}`;
}

export function buildProfileCopyDraft(
  profile: ProfileDef,
  copySuffix: string,
): ProfileDraft {
  return {
    label: copyProfileLabel(profile.label, copySuffix),
    provider: profile.provider,
    auth_mode: profile.auth_mode,
    api_types: [...profile.api_types],
    credentials: { ...profile.credentials },
    overrides: structuredClone(profile.overrides),
    provider_settings: structuredClone(profile.provider_settings ?? {}),
  };
}
