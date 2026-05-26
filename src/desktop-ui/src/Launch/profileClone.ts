import type { ProfileDef, ProfileDraft } from "./types";

function parseCopyLabel(label: string, copySuffix: string) {
  const trimmed = label.trim();
  const suffix = copySuffix.trim() || "Copy";
  if (!trimmed) return { base: suffix, copyNumber: 0 };

  const escapedSuffix = suffix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const existingCopyPattern = new RegExp(
    `^(.*)\\s+${escapedSuffix}(?:\\s+(\\d+))?$`,
  );
  const existing = trimmed.match(existingCopyPattern);
  if (!existing) return { base: trimmed, copyNumber: 0 };

  return {
    base: existing[1]?.trim() || trimmed,
    copyNumber: Number(existing[2] ?? "1"),
  };
}

function formatCopyLabel(base: string, copySuffix: string, copyNumber: number) {
  const suffix = copySuffix.trim() || "Copy";
  return copyNumber <= 1
    ? `${base} ${suffix}`
    : `${base} ${suffix} ${copyNumber}`;
}

export function copyProfileLabel(
  label: string,
  copySuffix: string,
  existingLabels: readonly string[] = [],
): string {
  const suffix = copySuffix.trim() || "Copy";
  const existing = new Set(
    existingLabels
      .map((existingLabel) => existingLabel.trim())
      .filter(Boolean),
  );
  const parsed = parseCopyLabel(label, suffix);
  let candidate = formatCopyLabel(parsed.base, suffix, parsed.copyNumber + 1);

  while (existing.has(candidate)) {
    const next = parseCopyLabel(candidate, suffix);
    candidate = formatCopyLabel(next.base, suffix, next.copyNumber + 1);
  }

  return candidate;
}

export function buildProfileCopyDraft(
  profile: ProfileDef,
  copySuffix: string,
  existingLabels: readonly string[] = [],
): ProfileDraft {
  return {
    label: copyProfileLabel(profile.label, copySuffix, existingLabels),
    provider: profile.provider,
    auth_mode: profile.auth_mode,
    api_types: [...profile.api_types],
    credentials: { ...profile.credentials },
    overrides: structuredClone(profile.overrides),
    provider_settings: structuredClone(profile.provider_settings ?? {}),
  };
}
