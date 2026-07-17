import { invoke } from "@tauri-apps/api/core";

export type SettingsSnapshot<T> = {
  settings: T;
  revision: string;
};

export type SettingsPatchOperation =
  | { op: "add"; path: string; value: unknown }
  | { op: "remove"; path: string };

export type SettingsPatch = SettingsPatchOperation[];

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function pointerSegment(key: string): string {
  return key.replaceAll("~", "~0").replaceAll("/", "~1");
}

function valuesEqual(base: unknown, desired: unknown): boolean {
  if (Object.is(base, desired)) return true;
  if (Array.isArray(base) && Array.isArray(desired)) {
    return JSON.stringify(base) === JSON.stringify(desired);
  }
  return false;
}

function appendDifference(
  patch: SettingsPatch,
  path: string,
  base: unknown,
  desired: unknown,
): void {
  if (valuesEqual(base, desired)) return;

  if (isObject(base) && isObject(desired)) {
    const keys = new Set([...Object.keys(base), ...Object.keys(desired)]);
    for (const key of keys) {
      const childPath = `${path}/${pointerSegment(key)}`;
      const baseHasKey = Object.hasOwn(base, key);
      const desiredHasKey = Object.hasOwn(desired, key);
      if (!desiredHasKey || desired[key] === undefined) {
        if (baseHasKey) patch.push({ op: "remove", path: childPath });
        continue;
      }
      if (!baseHasKey) {
        patch.push({ op: "add", path: childPath, value: desired[key] });
        continue;
      }
      appendDifference(patch, childPath, base[key], desired[key]);
    }
    return;
  }

  patch.push({ op: "add", path, value: desired });
}

export function createSettingsPatch<T>(base: T, desired: T): SettingsPatch {
  if (!isObject(base) || !isObject(desired)) {
    throw new Error("settings must remain a JSON object");
  }
  const patch: SettingsPatch = [];
  appendDifference(patch, "", base, desired);
  return patch;
}

export function saveSettingsPatch<T>(base: T, desired: T): Promise<SettingsSnapshot<T>> {
  return invoke<SettingsSnapshot<T>>("save_settings", {
    patch: createSettingsPatch(base, desired),
  });
}
