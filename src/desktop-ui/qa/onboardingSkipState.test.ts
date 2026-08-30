import { expect, test } from "bun:test";

import { hasSkippedReport } from "../src/Onboarding/lib/checkReports";
import type { StartkitItemReport } from "../src/Onboarding/types";

function report(status: StartkitItemReport["status"]): StartkitItemReport {
  return {
    id: "essentials.node",
    label: "Node.js",
    group: "computer",
    category: "essentials",
    status,
    actions: [],
    secret: false,
  };
}

test("does not rescan when undoing a local skip before installation", () => {
  expect(hasSkippedReport([report("missing")], "essentials.node")).toBeFalse();
});

test("rescans when undoing a skip committed by an installation run", () => {
  expect(hasSkippedReport([report("skipped")], "essentials.node")).toBeTrue();
});
