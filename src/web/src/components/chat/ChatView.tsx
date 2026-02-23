"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "./Conversation";
import { Message, MessageContent } from "./Message";
import { MessageResponse } from "./MessageResponse";
import { ChatInput } from "./ChatInput";

import { getWebSocketUrl } from "@/lib/ws-url";

/** Max number of previous messages to include as context (client-side context memory). */
const CONTEXT_MESSAGE_LIMIT = 20;

function buildPromptWithContext(messages: ChatMessage[], newUserMessage: string): string {
  if (messages.length === 0) return newUserMessage;
  const recent = messages.slice(-CONTEXT_MESSAGE_LIMIT);
  const lines = recent.map((m) => (m.role === "user" ? `User: ${m.content}` : `Assistant: ${m.content}`));
  return `Previous conversation:\n\n${lines.join("\n\n")}\n\nUser: ${newUserMessage}`;
}

export type ChatMessage = { role: "user" | "assistant"; content: string };

export function ChatView() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [connected, setConnected] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);

  // Connect on mount, close on unmount
  useEffect(() => {
    const ws = new WebSocket(getWebSocketUrl("/ws/chat"));
    wsRef.current = ws;

    ws.onopen = () => setConnected(true);
    ws.onclose = () => {
      setConnected(false);
      setStreaming(false);
    };
    ws.onerror = () => setConnected(false);

    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      const s = event.data as string;
      try {
        const j = JSON.parse(s);
        if (j?.done === true) {
          setStreaming(false);
          return;
        }
        if (typeof j?.error === "string") {
          setMessages((prev) => {
            const last = prev[prev.length - 1];
            if (last?.role === "assistant") {
              const next = [...prev];
              next[next.length - 1] = {
                ...last,
                content: last.content + (last.content ? "\n\n" : "") + `Error: ${j.error}`,
              };
              return next;
            }
            return [...prev, { role: "assistant", content: `Error: ${j.error}` }];
          });
          setStreaming(false);
          return;
        }
      } catch {
        // not JSON: streamed text chunk
      }
      setMessages((prev) => {
        if (prev.length === 0) return [{ role: "assistant", content: s }];
        const last = prev[prev.length - 1];
        if (last.role !== "assistant") {
          return [...prev, { role: "assistant", content: s }];
        }
        const next = [...prev];
        next[next.length - 1] = { ...last, content: last.content + s };
        return next;
      });
    };

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, []);

  const sendMessage = useCallback(() => {
    const text = input.trim();
    if (!text || !wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;

    const prompt = buildPromptWithContext(messages, text);
    setInput("");
    setMessages((prev) => [
      ...prev,
      { role: "user", content: text },
      { role: "assistant", content: "" },
    ]);
    setStreaming(true);
    wsRef.current.send(prompt);
  }, [input, messages]);

  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      <Conversation className="flex-1">
        <ConversationContent>
          {messages.length === 0 ? (
            <ConversationEmptyState
              title="Chat with Claude"
              description="Backed by Claude CLI headless (-p). Send a message to start."
            />
          ) : (
            messages.map((msg, i) => (
              <Message key={i} from={msg.role}>
                <MessageContent
                  className={
                    msg.role === "user"
                      ? "rounded-lg bg-primary/15 px-4 py-3 text-foreground"
                      : "rounded-lg bg-muted/50 px-4 py-3 text-foreground"
                  }
                >
                  {msg.role === "user" ? (
                    <p className="whitespace-pre-wrap text-sm">{msg.content}</p>
                  ) : (
                    <MessageResponse
                      content={msg.content}
                      isStreaming={streaming && i === messages.length - 1}
                    />
                  )}
                </MessageContent>
              </Message>
            ))
          )}
        </ConversationContent>
        <ConversationScrollButton />
      </Conversation>

      <ChatInput
        value={input}
        onChange={setInput}
        onSubmit={sendMessage}
        disabled={!connected}
        isStreaming={streaming}
        placeholder={connected ? "Message Claude…" : "Connecting…"}
      />
    </div>
  );
}
