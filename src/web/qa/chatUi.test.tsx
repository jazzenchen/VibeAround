import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { I18nProvider } from "@va/i18n";
import {
  ChatInput,
  ChatMessageList,
  ChatTurnDisplay,
  PendingPermissions,
} from "../src/components/chat/chatUi";

function render(component: React.ReactNode) {
  return renderToStaticMarkup(<I18nProvider>{component}</I18nProvider>);
}

test("shared chat display components render from plain props", () => {
  const transcript = render(
    <ChatMessageList
      messages={[{ role: "user", content: "Review this section" }]}
      streaming={false}
      agentLabel="Agent"
      displaySettings={{ showThinking: true, showTools: true }}
    />,
  );
  const turn = render(
    <ChatTurnDisplay
      message={{
        role: "assistant",
        content: "",
        activities: [
          {
            id: "read-file",
            kind: "tool",
            label: "Read file",
            status: "active",
            active: true,
          },
        ],
      }}
      isStreaming
      displaySettings={{ showThinking: true, showTools: true }}
    />,
  );
  const permissions = render(
    <PendingPermissions
      permissions={[
        {
          requestId: "permission-1",
          request: {
            toolCall: { title: "Edit README" },
            options: [{ optionId: "allow", name: "Allow" }],
          },
        },
      ]}
      onRespond={() => {}}
      onCancel={() => {}}
    />,
  );

  expect(transcript).toContain("Review this section");
  expect(turn).toContain("Read file");
  expect(permissions).toContain("Edit README");
  expect(permissions).toContain("Allow");
});

test("shared composer can omit dashboard commands", () => {
  const props = {
    value: "",
    onChange: () => {},
    onSubmit: () => {},
  };

  const dashboardComposer = render(<ChatInput {...props} />);
  const compactComposer = render(
    <ChatInput
      {...props}
      showCommands={false}
      compact
      contextContent={<span>Review note</span>}
    />,
  );

  expect(dashboardComposer).toContain('aria-label="Commands"');
  expect(compactComposer).not.toContain('aria-label="Commands"');
  expect(compactComposer).toContain('aria-label="Send"');
  expect(compactComposer).toContain("Review note");
});

test("shared composer can submit contextual review notes without prompt text", () => {
  const composer = render(
    <ChatInput
      value=""
      onChange={() => {}}
      onSubmit={() => {}}
      showCommands={false}
      contextCanSubmit
      contextContent={<span>README review note</span>}
    />,
  );

  expect(composer).toContain("README review note");
  const sendButton = composer.match(/<button[^>]*aria-label="Send"[^>]*>/)?.[0];
  expect(sendButton).toBeDefined();
  expect(sendButton).not.toMatch(/\sdisabled(?:=|>)/);
});
