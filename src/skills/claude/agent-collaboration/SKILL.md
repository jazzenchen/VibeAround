---
name: agent-collaboration
description: Initialize and wait for VibeAround subagents in a multi-agent coding turn. Use when the user's message starts with "subagent=", especially "subagent=parallel".
---

# VibeAround Agent Collaboration

Initialize a VibeAround multi-agent turn from the current host agent, then wait for subagent reports before finalizing.

## When to Use

- The user message starts with `subagent=`
- The first supported mode is `subagent=parallel`

## Protocol

All host-to-subagent and subagent-to-host control messages must use `va-agent-protocol`.

VibeAround intercepts protocol envelopes in the thread. The raw envelope is hidden from the web UI and forwarded internally to the target agent. The protocol envelope must be the final content in the assistant message; do not write prose after it.

Use this envelope for structured messages:

```xml
<va-agent-protocol>
{
  "protocol": "va-agent-protocol",
  "kind": "assignment",
  "turn_id": "<multi_agent_turn_id>",
  "to_agent_id": "<subagent_guid>",
  "task": "<clear task for this subagent>",
  "context": "<only the relevant context>"
}
</va-agent-protocol>
```

Subagents must report back with:

```xml
<va-agent-protocol>
{
  "protocol": "va-agent-protocol",
  "kind": "report",
  "turn_id": "<multi_agent_turn_id>",
  "from_agent_id": "<subagent_guid>",
  "status": "completed",
  "summary": "<what changed or what was found>",
  "files_changed": [],
  "tests": [],
  "notes": []
}
</va-agent-protocol>
```

## Initialize Parallel Subagents

Call the VibeAround MCP tool:

```
Tool: initialize_subagents
Server: vibearound
Arguments:
  thread_id: "<value of $VIBEAROUND_THREAD_ID>"
  cwd: "<current working directory>"
  mode: "parallel"
  agents:
    - name: "<host-chosen display name, e.g. John Planner>"
      agent_kind: "codex"
      task: "<parallel task>"
    - name: "<host-chosen display name, e.g. Maya Reviewer>"
      agent_kind: "codex"
      task: "<parallel task>"
```

Then wait for the turn to finish:

```
Tool: wait_for_subagents
Server: vibearound
Arguments:
  thread_id: "<value of $VIBEAROUND_THREAD_ID>"
  turn_id: "<turn id returned by initialize_subagents>"
```

Rules:

- Choose concise human names for subagents. Names are display aliases; the MCP tool returns GUID agent IDs.
- For `parallel`, default to exactly 2 subagents.
- Use `codex` for subagents by default, even when the host is Claude Code.
- Only create more than 2 subagents or use another `agent_kind` when the user explicitly asks.
- For `parallel`, split the user's request into independent tasks that can run in separate git worktrees.
- VibeAround handles missing Git repositories by running `git init` and creating an empty initial commit when needed. If Git itself is missing, VibeAround attempts a platform install before reporting an error.
- Do not merge or clean up worktrees automatically. The host agent decides after reviewing results.
- If VibeAround reports a dirty workspace or worktree creation error, tell the user and stop the multi-agent turn.
- VibeAround injects subagent role/system guidance at session startup. Keep assignments focused on the task and relevant context.

After `initialize_subagents` returns, do not produce a final answer yet. Call `wait_for_subagents`, review the returned reports and any visible subagent messages, then synthesize the host answer.

## Continue Delegating

After `initialize_subagents` returns, the host can continue delegating to an existing subagent by emitting an assignment envelope in the host response. VibeAround intercepts the envelope and sends it to the target subagent:

```xml
<va-agent-protocol>
{
  "protocol": "va-agent-protocol",
  "kind": "assignment",
  "turn_id": "<multi_agent_turn_id>",
  "to_agent_id": "<subagent_guid>",
  "task": "<follow-up task>",
  "context": "<only what changed since the previous assignment>"
}
</va-agent-protocol>
```

Rules:

- Use exactly the `turn_id` and `to_agent_id` returned by `initialize_subagents`.
- Include a non-empty `task`.
- Put the envelope at the end of the assistant message. Do not write anything after the closing `</va-agent-protocol>` tag.
- Do not use MCP for follow-up delegation. The protocol envelope is the control path.
