import { expect, test } from "bun:test";

import {
  hasPreviewHelperDragStarted,
  nearestPreviewHelperCorner,
} from "../src/preview/previewHelperPosition";

test("Preview helper distinguishes a click from a six-pixel drag", () => {
  expect(hasPreviewHelperDragStarted({ x: 10, y: 10 }, { x: 13, y: 14 })).toBe(false);
  expect(hasPreviewHelperDragStarted({ x: 10, y: 10 }, { x: 16, y: 10 })).toBe(true);
});

test("Preview helper snaps by its center to the nearest viewport corner", () => {
  const viewport = { width: 1000, height: 800 };
  const size = { width: 120, height: 40 };

  expect(nearestPreviewHelperCorner({ ...size, left: 20, top: 20 }, viewport))
    .toBe("top-left");
  expect(nearestPreviewHelperCorner({ ...size, left: 860, top: 20 }, viewport))
    .toBe("top-right");
  expect(nearestPreviewHelperCorner({ ...size, left: 20, top: 720 }, viewport))
    .toBe("bottom-left");
  expect(nearestPreviewHelperCorner({ ...size, left: 860, top: 720 }, viewport))
    .toBe("bottom-right");
  expect(nearestPreviewHelperCorner({ ...size, left: 450, top: 20 }, viewport))
    .toBe("top-right");
});
