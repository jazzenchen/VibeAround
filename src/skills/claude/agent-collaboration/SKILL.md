---
name: agent-collaboration
description: Initialize VibeAround subagents for a multi-agent coding turn. Use when the user's message starts with "subagent=", especially "subagent=parallel".
---

# VibeAround Agent Collaboration

Initialize a VibeAround multi-agent turn from the current host agent.

## When to Use

- The user message starts with `subagent=`
- The first supported mode is `subagent=parallel`

## Protocol

All host-to-subagent and subagent-to-host messages must use `va-agent-protocol`.

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
  "artifacts": [],
  "questions_for_host": []
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
      agent_kind: "<claude or codex unless the user asks otherwise>"
      task: "<parallel task>"
```

Rules:

- Choose concise human names for subagents. Names are display aliases; the MCP tool returns GUID agent IDs.
- For `parallel`, split the user's request into independent tasks that can run in separate git worktrees.
- Do not merge or clean up worktrees automatically. The host agent decides after reviewing results.
- If VibeAround reports a dirty workspace or worktree creation error, tell the user and stop the multi-agent turn.

After the tool returns, summarize the created subagents, their branches, and their worktrees.
