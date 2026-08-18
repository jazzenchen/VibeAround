import { expect, test } from "bun:test";

import { parseLaunchArgInput } from "../src/Launch/agentLaunchArgs";

test("parseLaunchArgInput splits shell-like command fragments", () => {
  expect(parseLaunchArgInput("--sandbox danger-full-access")).toEqual({
    args: ["--sandbox", "danger-full-access"],
    error: null,
  });
  expect(parseLaunchArgInput('--model "gpt 5" -c key=value')).toEqual({
    args: ["--model", "gpt 5", "-c", "key=value"],
    error: null,
  });
  expect(parseLaunchArgInput("--flag a\\ b 'literal value'")).toEqual({
    args: ["--flag", "a b", "literal value"],
    error: null,
  });
});

test("parseLaunchArgInput reports invalid fragments", () => {
  expect(parseLaunchArgInput('"unterminated')).toEqual({
    args: [],
    error: "unterminatedQuote",
  });
  expect(parseLaunchArgInput("dangling\\")).toEqual({
    args: [],
    error: "danglingEscape",
  });
  expect(parseLaunchArgInput("--one\n--two")).toEqual({
    args: [],
    error: "lineBreak",
  });
});
