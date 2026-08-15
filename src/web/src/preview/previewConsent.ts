import type { PreviewItem } from "./previewTypes";

const SERVER_PREVIEW_CONSENT_PREFIX = "vibearound.preview.server-consent.";

type PreviewConsentStorage = Pick<Storage, "getItem" | "setItem">;

export function serverPreviewNeedsConsent(
  preview: PreviewItem,
  storage: PreviewConsentStorage,
): boolean {
  return (
    preview.kind === "server" &&
    storage.getItem(`${SERVER_PREVIEW_CONSENT_PREFIX}${preview.slug}`) !== "1"
  );
}

export function rememberServerPreviewConsent(
  preview: PreviewItem,
  storage: PreviewConsentStorage,
): void {
  if (preview.kind === "server") {
    storage.setItem(`${SERVER_PREVIEW_CONSENT_PREFIX}${preview.slug}`, "1");
  }
}
