import { invoke } from "@tauri-apps/api/core";

export type SettingsSnapshot<T> = {
  settings: T;
  revision: string;
};

const UNCHANGED = Symbol("unchanged");

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function mergePatchDifference(base: unknown, desired: unknown): unknown | typeof UNCHANGED {
  if (Object.is(base, desired)) return UNCHANGED;

  if (isObject(base) && isObject(desired)) {
    const patch: Record<string, unknown> = {};
    const keys = new Set([...Object.keys(base), ...Object.keys(desired)]);
    for (const key of keys) {
      if (!(key in desired) || desired[key] === undefined) {
        patch[key] = null;
        continue;
      }
      if (!(key in base)) {
        patch[key] = desired[key];
        continue;
      }
      const difference = mergePatchDifference(base[key], desired[key]);
      if (difference !== UNCHANGED) patch[key] = difference;
    }
    return Object.keys(patch).length === 0 ? UNCHANGED : patch;
  }

  if (Array.isArray(base) && Array.isArray(desired)) {
    return JSON.stringify(base) === JSON.stringify(desired) ? UNCHANGED : desired;
  }

  return desired;
}

export function createSettingsPatch<T>(base: T, desired: T): Record<string, unknown> {
  const patch = mergePatchDifference(base, desired);
  if (patch === UNCHANGED) return {};
  if (!isObject(patch)) {
    throw new Error("settings must remain a JSON object");
  }
  return patch;
}

export function saveSettingsPatch<T>(base: T, desired: T): Promise<SettingsSnapshot<T>> {
  return invoke<SettingsSnapshot<T>>("save_settings", {
    patch: createSettingsPatch(base, desired),
  });
}
