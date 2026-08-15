export type PreviewChatMode = "floating" | "impact";
export type PreviewChatSide = "left" | "right";

export const MIN_PREVIEW_CHAT_WIDTH = 280;
export const MAX_PREVIEW_CHAT_WIDTH = 560;

export function clampPreviewChatWidth(width: number) {
  return Math.min(
    MAX_PREVIEW_CHAT_WIDTH,
    Math.max(MIN_PREVIEW_CHAT_WIDTH, width),
  );
}

export function resizePreviewChatWidth(
  width: number,
  boundaryMovement: number,
  side: PreviewChatSide,
) {
  return clampPreviewChatWidth(
    width + (side === "left" ? boundaryMovement : -boundaryMovement),
  );
}
