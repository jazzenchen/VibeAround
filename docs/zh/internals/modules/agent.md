# Module: agent

`src/core/src/agent/`：到一个编程 CLI 的 live ACP connection，以及正确启动它所需的一切：launch rendering、config injection、install helpers。

## 职责

把单个 agent subprocess 包在 typed handle（`Agent`）后面，通过 ACP 通信，并准备它运行的地面：profile-derived env/args、写入 agent 全局 config 的 VibeAround MCP + skills、启动 session 语义（fresh / load / resume）。

## 关键类型

| Type | File | Role |
|---|---|---|
| `Agent` | `runtime.rs` | Handle：`prompt`、`cancel`、`shutdown`，通过 supervisor spawn（policy `Never`）；仅 stdio ACP，在出现第二种 transport 前没有 transport trait |
| `AgentClientHandler` | `runtime.rs` | Southbound callback trait：`session_notification`、`request_permission`、`prompt_finished`；由 `channels::bridge_handler` 和 subagent handler 实现 |
| `AcpAgentBridge` | `bridge.rs` | ProcessBridge impl：驱动 ACP connection，处理 startup session attach/fallback |
| `StartupSession` | `runtime.rs` | Fresh vs resume-by-id 启动语义 |
| `launch` | `launch.rs` | Hosted + native launches 的 profile materialization（`DIRECT_PROFILE_ID`、credential env、profile-id env） |
| `mcp` / `skills` | `mcp.rs`, `skills.rs` | 按 agent 注入 global + project-scoped config（路径来自 registry 的 `global_config`） |
| `install` | `install.rs` | Agent CLI / ACP adapter 安装（registry 中的 npm packages） |

## 交互

- **← workspace：** `ThreadRuntime::ensure_agent` 是主要调用方；subagent spawning 使用同一个 `Agent::spawn`。
- **→ process：** spawn/shutdown 委托给 `Supervisor`。
- **→ profiles：** `launch.rs` 拉取已渲染凭据和 bridge URLs。
- **→ resources：** 所有 agent identity（ids、aliases、adapter packages、config paths）都来自 embedded registry，逻辑里不要 hardcode agent id。

## 不变量：不要破坏

1. **Crash 要显性暴露，不自动修复**：restart policy 是 `Never`；owning thread 决定是否 respawn。不要在这里加 silent retry。
2. **Startup-session fallback 要清掉 stale id**：如果 resume 失败且 bridge fallback 到 fresh agent，记录的 candidate session id 必须清掉，后续才会创建真实 session，否则 prompt 会指向死 session。
3. **Config injection 幂等且可逆**：MCP/skill 写入带 VibeAround-managed 标记，daemon down 时 launch-time cleanup 能移除。
4. **Registry-driven identity**：新增 agent 应是 `agents.json` 变更（adapter package、pty command、config paths），不是新 match arms。尽量保持这一点。

## 已知技术债

- `profiles/bridge_launch.rs`（由 `launch.rs` 使用）hardcode launch-target match arms（`"claude" | "codex" | …`）。目前可接受，列入 M7 URL-shape consolidation 后的 catalog-driven cleanup。

---

*Source anchors: `src/core/src/agent/` (runtime, bridge, launch, mcp, skills, install), `src/resources/agents.json`, `src/core/src/resources.rs`.*
*Last verified: v0.7.11*

<sub>[◀ Module: process](process.md) · [文档索引](../../README.md) · [Module: profiles ▶](profiles.md)</sub>
