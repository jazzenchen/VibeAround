# Flow: Agent 启动

从点击 “Launch”（或 `va launch`）到 agent CLI 在你的终端里运行。塑造这个流程的架构规则是：provider/runtime preparation 发生在**原生 launcher 之前**；`va-launch` 本身是独立二进制，必须在没有 desktop app 的情况下工作。机制细节，例如 env 层、各 OS 脚本、参数来源，在 [launch 子系统深挖](../launch.md) 里。

## 逐跳

```text
UI / CLI ──1─► provider prep ──2─► launch profile JSON ──3─► va-launch
                                                               │4 validate
                                                               │5 project integrations
                                                               ▼6
                                                        terminal spawn
                                                               │7
                                                        CLI ──► /va/local-api (models)
```

**1. Selection。** Desktop Launch screen 或 `va launch --profile <name>` 选择 agent、workspace、model profile 和 terminal preference。

**2. Provider prep（launcher 上游）。** 对 bridged profile，profile 会渲染成具体启动材料：env vars、每个 agent 的 config overlays（Codex `model_providers` args、Claude settings、Gemini/opencode/pi variants），以及 scoped to this launch 的 local-api base URLs。`direct` profile 跳过全部这些准备。Desktop-app targets 会得到自己的 overlay variant。
→ `src/core/src/agent/launch.rs`, `src/core/src/profiles/bridge_launch.rs`, `render.rs`

**3. Launch profile JSON。** 所有内容序列化成 schema-v1 launch profile：agent、workspace、terminal、command/executable override、env、args、window label。保存的 profiles 位于 `~/.vibearound/launch/profiles/`；desktop 会写一个物化的临时 profile。未知字段会被拒绝；如果 producer 把 provider profile 误交给 launcher，会明确失败。
→ launch profile schema (internal notes), `src/launcher/`

**4. va-launch validates。** CLI/desktop **执行 sibling `va-launch` binary**（不是进程内 launcher）。它验证 workspace，解析 agent executable（显式 path → `agents.json` → PATH scan，结果缓存），并验证 terminal choice。`--dry-run` 在这里停止并打印 plan。
→ `src/launcher/` (resolution order), `~/.vibearound/agents.json`

**5. Project integrations。** va-launch 探测本地 daemon health endpoint：
- **Daemon up** → 为这个 agent/workspace 安装 project-scoped MCP config 和 skills（遵守 [`integrations.*` settings](../../reference/configuration.md#settingsjson)）。Desktop-app targets 会安装其**伴随 CLI**的 integrations：`claude-desktop` → `claude`，`codex-desktop` → `codex`。
- **Daemon down** → *移除* VibeAround-managed project integrations，避免死掉的 MCP server 留在配置里。
→ `src/launcher/` (health probe), `src/core/src/agent/{mcp,skills}.rs`

**6. Terminal spawn。** Agent 在选定终端里打开（Terminal.app/iTerm2、PowerShell 或检测到的 Linux terminal）；desktop-app targets 则通过 `open -a` / `Start-Process`。终端启动后 va-launch 的工作结束，CLI 进程属于你，不属于 daemon。

**7. Runtime relationship。** Bridged CLI 现在调用 `127.0.0.1:12358` 获取模型（[bridge 请求流程](bridge-request.md)），并可使用注入的 MCP tools（handover、previews、subagents）。它的原生 sessions 会被 daemon 发现，并可从其它所有界面 resume（[交接流程](handover.md)）。

## 失败行为

| 失败 | 结果 |
|---|---|
| 找不到 executable | 在启动任何东西前验证失败；清掉过期的 `agents.json` entry 可强制重新扫描 |
| Workspace 不存在 | 验证失败 |
| Launch 时 daemon down | 启动继续；integrations 被移除；bridged model calls 在 daemon 恢复前失败 |
| Terminal not found（Linux） | Launch error 会列出尝试过的内容 |
| JSON shape 错误 | Schema rejection（unknown fields）；producer bug 会立刻暴露 |

---

*Source anchors: `src/launcher/` (va-launch), `src/core/src/agent/launch.rs` + `src/core/src/profiles/bridge_launch.rs` (provider prep), `src/core/src/agent/{mcp,skills}.rs` (integrations), `src/cli/src/args.rs` (va launch), internal boundary notes in `.docs/va-launch-architecture.md`.*
*Last verified: v0.7.11*

<sub>[◀ Flow: Bridge 请求](bridge-request.md) · [文档索引](../../README.md) · [Flow: 交接 ▶](handover.md)</sub>
