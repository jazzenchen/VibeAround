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
import type { ToolType } from "@/lib/terminal-types";
import { toolThemes } from "@/lib/terminal-types";
import type { AgentInfo } from "@/api/agents";

/** Map agent id to ToolType for theming. Falls back to "generic". */
function agentIdToToolType(id: string): ToolType {
  if (id in toolThemes) return id as ToolType;
  return "generic";
}

/** Capitalize first letter. */
function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

export type ChatMessage = {
  role: "user" | "assistant" | "system";
  content: string;
  progress?: string;
};

export function ChatView() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [connected, setConnected] = useState(false);
  const [streaming, setStreaming] = useState(false);

  // Agent state
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [currentAgent, setCurrentAgent] = useState<string>("claude");
  const wsRef = useRef<WebSocket | null>(null);

  const toolType = agentIdToToolType(currentAgent);
  const agentLabel = capitalize(currentAgent);

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

      let j: Record<string, unknown>;
      try {
        j = JSON.parse(s);
      } catch {
        appendToAssistant(s);
        return;
      }

      // {"type":"config","agents":[...],"default_agent":"claude"} — agent config push on connect
      if (j.type === "config" && Array.isArray(j.agents)) {
        setAgents(j.agents as AgentInfo[]);
        if (typeof j.default_agent === "string") {
          setCurrentAgent(j.default_agent as string);
        }
        return;
      }

      // {"type":"agent_switched","agent":"opencode"} — backend confirmed agent switch
      if (j.type === "agent_switched" && typeof j.agent === "string") {
        setCurrentAgent(j.agent as string);
        return;
      }

      // {"type":"system_text","text":"..."} — standalone system message
      if (j.type === "system_text" && typeof j.text === "string") {
        setMessages((prev) => [...prev, { role: "system", content: j.text as string }]);
        setStreaming(false);
        return;
      }

      // {"done":true} — stream finished
      if (j.done === true) {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last?.role === "assistant" && last.progress) {
            const next = [...prev];
            next[next.length - 1] = { ...last, progress: undefined };
            return next;
          }
          return prev;
        });
        setStreaming(false);
        return;
      }

      // {"error":"..."} — error
      if (typeof j.error === "string") {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last?.role === "assistant") {
            const next = [...prev];
            next[next.length - 1] = {
              ...last,
              content: last.content + (last.content ? "\n\n" : "") + `Error: ${j.error}`,
              progress: undefined,
            };
            return next;
          }
          return [...prev, { role: "assistant", content: `Error: ${j.error}` }];
        });
        setStreaming(false);
        return;
      }

      // {"progress":"Thinking..."} — progress indicator
      if (typeof j.progress === "string") {
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last?.role === "assistant") {
            const next = [...prev];
            next[next.length - 1] = { ...last, progress: j.progress as string };
            return next;
          }
          return prev;
        });
        return;
      }

      // {"text":"..."} — text content to append
      if (typeof j.text === "string") {
        appendToAssistant(j.text as string);
        return;
      }
    };

    function appendToAssistant(text: string) {
      if (!text) return;
      setMessages((prev) => {
        if (prev.length === 0) return [{ role: "assistant", content: text }];
        const last = prev[prev.length - 1];
        if (last.role !== "assistant") {
          return [...prev, { role: "assistant", content: text }];
        }
        const next = [...prev];
        next[next.length - 1] = { ...last, content: last.content + text, progress: undefined };
        return next;
      });
    }

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, []);

  const sendMessage = useCallback(() => {
    const text = input.trim();
    if (!text || !wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;

    setInput("");
    setMessages((prev) => [
      ...prev,
      { role: "user", content: text },
      { role: "assistant", content: "" },
    ]);
    setStreaming(true);
    wsRef.current.send(JSON.stringify({ type: "message", text }));
  }, [input]);

  const handleAgentChange = useCallback((agentId: string) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    setCurrentAgent(agentId);
    wsRef.current.send(JSON.stringify({ type: "message", text: `/agent ${agentId}` }));
  }, []);

  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      <Conversation className="flex-1">
        <ConversationContent>
          {messages.length === 0 ? (
            <ConversationEmptyState
              title={`Chat with ${agentLabel}`}
              description="Send a message to start."
            />
          ) : (
            messages.map((msg, i) => (
              <Message key={i} from={msg.role}>
                <MessageContent
                  className={
                    msg.role === "user"
                      ? "rounded-lg bg-primary/15 px-4 py-3 text-foreground"
                      : msg.role === "system"
                        ? "rounded-lg border border-border/60 bg-muted/20 px-4 py-3 text-muted-foreground"
                        : "rounded-lg bg-muted/50 px-4 py-3 text-foreground"
                  }
                >
                  {msg.role === "user" ? (
                    <p className="whitespace-pre-wrap text-sm">{msg.content}</p>
                  ) : msg.role === "system" ? (
                    <p className="whitespace-pre-wrap text-xs font-mono leading-5">{msg.content}</p>
                  ) : (
                    <>
                      <MessageResponse
                        content={msg.content}
                        isStreaming={streaming && i === messages.length - 1}
                      />
                      {msg.progress && (
                        <span className="text-xs text-muted-foreground/60 font-mono animate-pulse">
                          {msg.progress}
                        </span>
                      )}
                    </>
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
        placeholder={connected ? `Message ${agentLabel}…` : "Connecting…"}
        targetLabel={agentLabel}
        targetTool={toolType}
        agents={agents}
        onAgentChange={handleAgentChange}
      />
    </div>
  );
}
