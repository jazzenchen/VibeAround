export interface PreviewItem {
  slug: string;
  title: string;
  workspace: string;
  src: string;
}

export interface PreviewBootstrap {
  selectedSlug: string;
  previews: PreviewItem[];
}

export type PreviewAnchor = {
  kind: "text" | "element" | "region";
  text: string;
  heading?: string;
  startLine?: number;
  endLine?: number;
  page?: {
    path?: string;
    hash?: string;
    title?: string;
  };
  element?: {
    tag?: string;
    id?: string;
    testId?: string;
    role?: string;
    label?: string;
    selector?: string;
    text?: string;
  };
  region?: {
    width: number;
    height: number;
  };
};

export type PreviewFrameRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type PreviewReviewDraft = {
  id: string;
  anchor: PreviewAnchor;
  comment: string;
  screenshot?: PreviewScreenshot;
};

export type PreviewScreenshot = {
  blob: Blob;
  fileName: string;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function parsePreviewBootstrap(value: unknown): PreviewBootstrap | null {
  if (!isRecord(value) || typeof value.selectedSlug !== "string") return null;
  if (!Array.isArray(value.previews)) return null;

  const previews: PreviewItem[] = [];
  for (const item of value.previews) {
    if (
      !isRecord(item) ||
      typeof item.slug !== "string" ||
      typeof item.title !== "string" ||
      typeof item.workspace !== "string" ||
      typeof item.src !== "string"
    ) {
      return null;
    }
    previews.push({
      slug: item.slug,
      title: item.title,
      workspace: item.workspace,
      src: item.src,
    });
  }

  return { selectedSlug: value.selectedSlug, previews };
}

export function refreshedPreviewSlug(
  bootstrap: PreviewBootstrap,
  currentSlug: string,
): string | null {
  return (
    [currentSlug, bootstrap.selectedSlug].find((slug) =>
      bootstrap.previews.some((preview) => preview.slug === slug),
    ) ?? bootstrap.previews[0]?.slug ?? null
  );
}
