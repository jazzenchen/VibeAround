export const PREVIEW_SURFACE_DRAG_THRESHOLD = 6;

export type PreviewHelperCorner =
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";

type Point = {
  x: number;
  y: number;
};

type Rect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

type Viewport = {
  width: number;
  height: number;
};

export function hasPreviewHelperDragStarted(start: Point, current: Point) {
  return Math.hypot(current.x - start.x, current.y - start.y) >=
    PREVIEW_SURFACE_DRAG_THRESHOLD;
}

export function nearestPreviewHelperCorner(
  rect: Rect,
  viewport: Viewport,
): PreviewHelperCorner {
  const vertical = rect.top + rect.height / 2 < viewport.height / 2
    ? "top"
    : "bottom";
  const horizontal = rect.left + rect.width / 2 < viewport.width / 2
    ? "left"
    : "right";
  return `${vertical}-${horizontal}`;
}
