import { expect, test } from "bun:test";

import { pairingDestination } from "../src/lib/auth";

const ORIGIN = "https://preview.example";

test("pairing returns to the requested VibeAround route", () => {
  expect(
    pairingDestination("/va/preview/u/readme?view=compact#content", ORIGIN),
  ).toBe("/va/preview/u/readme?view=compact#content");
});

test("pairing rejects destinations outside the VibeAround app", () => {
  for (const destination of [
    null,
    "/outside",
    "//evil.example/path",
    "/\\evil.example/path",
    "https://evil.example/path",
    "javascript:alert(1)",
  ]) {
    expect(pairingDestination(destination, ORIGIN)).toBe("/va/");
  }
});
