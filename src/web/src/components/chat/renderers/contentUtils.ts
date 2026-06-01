import { browserBaseUrl } from "@va/client";
import { getAuthToken } from "@/lib/auth";

export function dataUrl(mimeType: string, data: string) {
  return data.startsWith("data:") ? data : `data:${mimeType};base64,${data}`;
}

export function fileNameFromUri(uri: string) {
  const clean = uri.split(/[?#]/)[0]?.replace(/[\\/]+$/, "") ?? uri;
  return clean.split(/[\\/]/).filter(Boolean).pop() ?? uri;
}

export function formatJson(value: unknown) {
  if (value === undefined || value === null) return "";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value >= 10 || unitIndex === 0 ? Math.round(value) : value.toFixed(1)} ${units[unitIndex]}`;
}

function previewJsonValue(value: unknown, maxStringLength: number): unknown {
  if (typeof value === "string") {
    if (value.length <= maxStringLength) return value;
    const bytes =
      typeof Blob !== "undefined"
        ? new Blob([value]).size
        : new TextEncoder().encode(value).byteLength;
    return `<string omitted: ${formatBytes(bytes)}>`;
  }
  if (Array.isArray(value)) {
    return value.map((item) => previewJsonValue(item, maxStringLength));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key,
        previewJsonValue(item, maxStringLength),
      ]),
    );
  }
  return value;
}

export function formatJsonPreview(value: unknown, maxStringLength = 4096) {
  return formatJson(previewJsonValue(value, maxStringLength));
}

export function lineCount(text: string | null | undefined) {
  if (!text) return 0;
  return text.split("\n").length;
}

export function proxiedFileUrl(
  uri: string | null | undefined,
  options: {
    name?: string | null;
    mimeType?: string | null;
    inline?: boolean;
  } = {},
) {
  if (!uri || uri.startsWith("data:") || uri.startsWith("blob:")) return uri ?? "";
  if (!isProxyableFileUri(uri)) return uri;

  const params = new URLSearchParams();
  params.set("uri", uri);
  if (options.name) params.set("name", options.name);
  if (options.mimeType) params.set("mime_type", options.mimeType);
  if (options.inline) params.set("inline", "true");
  const token = getAuthToken();
  if (token) {
    params.set("token", token);
  }
  return `${browserBaseUrl()}/api/chat/files/download?${params.toString()}`;
}

function isProxyableFileUri(uri: string) {
  return (
    uri.startsWith("file://") ||
    uri.startsWith("http://") ||
    uri.startsWith("https://") ||
    uri.startsWith("/")
  );
}
