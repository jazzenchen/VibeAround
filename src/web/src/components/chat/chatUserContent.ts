import type { ContentBlock } from "@agentclientprotocol/sdk";

import type { ChatAttachment } from "./chatTypes";

export function chatUserContentBlocks(
  text: string,
  attachments: ChatAttachment[],
): ContentBlock[] {
  const blocks: ContentBlock[] = [];
  if (text) blocks.push({ type: "text", text });
  blocks.push(
    ...attachments.map((attachment) => ({
      type: "resource_link" as const,
      name: attachment.name,
      title: attachment.name,
      mimeType: attachment.mimeType,
      size: attachment.size,
      uri: attachment.uri,
    })),
  );
  return blocks;
}
