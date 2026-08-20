import { expect, test } from "bun:test";

import {
  urlWithoutWebChatHandoff,
  webChatHandoffThreadId,
} from "../src/components/chat/webChatHandoff";

test("reads a workspace thread handoff from the dashboard URL", () => {
  expect(
    webChatHandoffThreadId("http://127.0.0.1:12358/va/?thread_id=wt_123&view=chat"),
  ).toBe("wt_123");
});

test("consuming a handoff preserves unrelated URL state", () => {
  expect(
    urlWithoutWebChatHandoff(
      "http://127.0.0.1:12358/va/?thread_id=wt_123&view=chat#latest",
    ),
  ).toBe("/va/?view=chat#latest");
});
