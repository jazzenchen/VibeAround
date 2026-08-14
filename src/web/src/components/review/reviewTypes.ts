export type ReviewTool = "element" | "region";

export type ReviewToolbarModel = {
  activeTool: ReviewTool | null;
  elementAvailable: boolean;
  regionAvailable: boolean;
  textSelectionAvailable: boolean;
  onToolChange: (tool: ReviewTool | null) => void;
};

export type ReviewAnchor = {
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

export type ReviewFrameRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type ReviewScreenshot = {
  blob: Blob;
  fileName: string;
};

export type ReviewDraft = {
  id: string;
  anchor: ReviewAnchor;
  comment: string;
  screenshot?: ReviewScreenshot;
};

export type ReviewEditor = {
  anchorId: string;
  selectionId?: string;
  draftId?: string;
  anchor: ReviewAnchor;
  rect: ReviewFrameRect;
  screenshot?: ReviewScreenshot;
};
