---
name: va-session
description: "Resolves the current VibeAround session ID for use by other skills. Use when another skill (va-preview, vibearound handover) needs session context, when the user asks 'what is my session ID', 'get session info', or 'check session status'."
---

# VibeAround Session ID

Resolves the current session ID. Other VibeAround skills call this when they need session context for preview, handover, or lifecycle management.

## How to Resolve

### Method 1: Via VibeAround env vars (preferred)

Check if `VIBEAROUND_CHANNEL_KIND` and `VIBEAROUND_CHAT_ID` are set. If yes:

```
Tool: get_session_id
Server: vibearound
Arguments:
  channel_kind: "<value of $VIBEAROUND_CHANNEL_KIND>"
  chat_id: "<value of $VIBEAROUND_CHAT_ID>"
```

Returns the exact session ID from VibeAround's internal state.

### Method 2: Fallback — agent-specific session files

If the env vars are not set (running outside VibeAround), resolve from local session metadata:

- **Claude Code**: Parse `~/.claude/history.jsonl` — find the last entry whose `project` matches the current working directory:
  ```bash
  tail -20 ~/.claude/history.jsonl | jq -r 'select(.project == "'$(pwd)'") | .sessionId' | tail -1
  ```
- **Codex**: Parse `~/.codex/history.jsonl` — extract from the last line:
  ```bash
  tail -1 ~/.codex/history.jsonl | jq -r '.session_id'
  ```
- **Gemini**: Use `/resume` to list recent sessions, then extract the session ID from the output.
- **Other agents**: Omit the session ID — the server will attempt auto-discovery.

## Return Value

Return the session ID string to the calling skill. If neither method succeeds, return nothing — callers handle the missing case gracefully.
