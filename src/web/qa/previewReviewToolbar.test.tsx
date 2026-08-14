import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { ReviewToolbar } from "../src/components/review/ReviewToolbar";

test("Preview review tools share one semantic toolbar across surfaces", () => {
  const markup = renderToStaticMarkup(
    <ReviewToolbar
      activeTool="element"
      elementAvailable
      regionAvailable={false}
      textSelectionAvailable
      onToolChange={() => {}}
    />,
  );

  expect(markup).toContain('aria-label="Preview review tools"');
  expect(markup).toContain("Element");
  expect(markup).toContain('aria-pressed="true"');
  expect(markup).toContain("Region");
  expect(markup).toContain("disabled");
  expect(markup).toContain("or select page text");
});
