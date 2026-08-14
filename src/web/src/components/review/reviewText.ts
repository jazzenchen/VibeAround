import type { ReviewAnchor } from "./reviewTypes";

export function reviewAnchorQuote(anchor: ReviewAnchor) {
  if (anchor.kind === "text" || anchor.kind === "region") return anchor.text;
  return (
    anchor.element?.text ??
    anchor.element?.label ??
    anchor.page?.title ??
    "Selected element"
  );
}

export function reviewAnchorLocation(anchor: ReviewAnchor) {
  const parts: string[] = [];
  if (anchor.page?.path) {
    parts.push(`${anchor.page.path}${anchor.page.hash ?? ""}`);
  }
  if (Number.isInteger(anchor.startLine)) {
    parts.push(
      anchor.endLine !== undefined && anchor.endLine > (anchor.startLine ?? 0)
        ? `lines ${anchor.startLine}–${anchor.endLine}`
        : `line ${anchor.startLine}`,
    );
  }
  if (anchor.heading) parts.push(anchor.heading);
  if (anchor.region) {
    parts.push(`${anchor.region.width} × ${anchor.region.height} screenshot`);
  }
  if (anchor.element) {
    parts.push(
      anchor.element.selector ??
        anchor.element.role ??
        anchor.element.tag ??
        "element",
    );
  }
  return parts.join(" · ") || "Selected content";
}
