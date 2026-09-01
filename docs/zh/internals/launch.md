# Launch 子系统

所有“启动 Agent 进程”的细节放在这里：三条启动路径、每条路径如何组装和注入环境变量、各 OS 的终端处理、参数来源，以及 desktop producer 和 CLI producer 的区别。一次原生启动的逐步走读在 [flows/native-launch](flows/native-launch.md)，用户指南在 [guides/agent-launch](../guides/agent-launch.md)。

## 三条启动路径

| 路径 | 触发方式 | 进程归属 | Env 注入机制 | 随 daemon 退出 |
|---|---|---|---|---|
| **原生启动** | desktop Launch / `va launch` | 你的终端应用 | 生成 shell 脚本（`export` / `$env:`） | 否 |
| **托管 ACP 启动** | IM / web chat prompt | daemon（supervisor） | `Command` spawn 时的 process env | 是 |
| **桌面应用目标** | 用 `claude-desktop` / `codex-desktop` Launch | 厂商 GUI app | `open --env`（macOS）/ `Start-Process`（Windows） | 否 |

三条路径都汇到同一套 profile rendering 代码。一个 Kimi profile 无论是 hosted、launched，还是打开 GUI app，都会产生同样的 `ANTHROPIC_BASE_URL`。

## Producers：desktop vs CLI

两个 producer 的边界相同：**把 launch-profile JSON 交给旁边的 `va-launch` 二进制执行**。两者都不会在进程内调用 launcher library。这里会遇到两个“profile”概念：**provider profile**（凭据 + 路由）被渲染成 **launch profile**（一次原生启动请求，[schema v1](../reference/configuration.md#launch-profile-json-schema-v1)）；`va-launch` 只看到后者，从不读取 provider 存储（JSON 里的 `profileId` 只是 metadata）。

| | Desktop | `va` CLI |
|---|---|---|
| Provider 准备 | 自己渲染 profile（`desktop/src/profiles/launcher/`：bridge overlay、codex/claude desktop 变体、resume plan），写入**物化的临时 launch profile JSON**，执行 `va-launch --profile-path <temp>` | 不做准备；`va launch --profile <name>` 读取 `~/.vibearound/launch/profiles/<name>.json` 中**已保存**的 profile，或读取任意 `--profile-path` 文件；只转发 `--profile`、`--profile-path`、`--dry-run`、`--json` |
| va-launch 二进制 | Tauri sidecar，随 app 放在可执行目录 | 和 `va` 一起随 npm package 发布 |
| Resume | Launch screen 选择 session；resume plan 把 agent 的 `resume_template`（`cd {cwd} && claude --resume {session_id}`）渲染成命令 | 保存的 profile 携带当时物化进去的命令 |
| Dev override | `VIBEAROUND_VA_LAUNCH_BIN` 让任一 producer 指向未打包的 launcher（仅 dev/test） | 同左 |

结论：**已保存**的 CLI launch profile 是静态的，持有创建时渲染出的 env（包括绑定到某个 `launch_id` 的 bridge URL）。Desktop 每次启动都会重新渲染。

## 环境变量组装，逐层叠加

Env 在 `va-launch` 上游构建，并放在 plan 的 `env` map 里（BTreeMap，去重，key 必须匹配 `[A-Za-z_][A-Za-z0-9_]*`）。

| 层 | 内容 | 适用于 |
|---|---|---|
| 1. 基础进程 env | 加强过的 login-shell env（`process/env.rs`，缓存一次），让 PATH 和用户 shell 一致 | hosted（原生启动继承终端自己的 login env） |
| 2. 身份 env | Hosted：`VIBEAROUND_CHANNEL_KIND`、`VIBEAROUND_CHAT_ID`、`VIBEAROUND_AGENT_KIND`、`VIBEAROUND_THREAD_ID`、`VIBEAROUND_WORKSPACE_ID`。Launch 物化：`VIBEAROUND_LAUNCH_ID`（每次渲染新 UUID）、`VIBEAROUND_PROFILE_ID`（归一化；无/default/off 为 `direct`）、`VIBEAROUND_LAUNCH_TARGET` | 所有 profile 驱动路径 |
| 3. Profile 凭据 + bridge | `bridge_launch.rs` 按 agent family 渲染变量：Claude → `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_BASE_URL`/`ANTHROPIC_MODEL`（加 custom-model-option 和 gateway-discovery flags）；Codex → `OPENAI_API_KEY` 加 `-c model_providers.…` **args** 而不是 env；Gemini → `GEMINI_API_KEY`、`GOOGLE_GEMINI_BASE_URL`、`GEMINI_MODEL`、`GEMINI_DEFAULT_AUTH_TYPE`。对于 bridged route，“API key” 是 scoped placeholder，base URL 指向 `127.0.0.1:12358/va/local-api/…`，真实 provider key 从不进入 env | bridged profiles |
| 4. Config 指针 | Profile 渲染 settings files 时，会写到 `~/.vibearound/profile-state/<profile-id>/…`，再用一个 `ConfigEnvTarget` env var 指过去：`Directory(env)` 或 `File { env, rel_path }`。**故意不用 `CODEX_HOME` / `CLAUDE_CONFIG_DIR`**，因为覆盖 agent home dir 会切断 CLI 对用户自身 sessions、plugins、skills 的访问 | 带 settings files 的 profiles |
| 5. Proxy | `append_settings_proxy_env`：只为 direct-to-provider route 导出 settings.json proxy（bridged route 走 daemon，daemon 自己应用 proxy） | direct provider routes |
| 6. 终端卫生 | 脚本层：`unset NO_COLOR`、`TERM=xterm-256color` fallback、`COLORTERM`/`CLICOLOR` 默认值；macOS 增加 terminal-update-suppression vars | 原生启动 |

## 各路径的注入机制

- **Hosted：** 直接在 spawned child 上设置 process env（`Command::env`），不写磁盘。
- **原生启动（macOS/Linux）：** `va-launch` 把一次性脚本写到 `$TMPDIR/vibearound/launch/scripts/script-<uuid>.{command,sh}`（mode 0700）。脚本会**先删除自己**（`rm -- "$0"`），然后逐个 `export` env（按 Unix shell 转义）、`cd` 到 workspace、`exec` 命令。Env value 会短暂经过文件系统；自删除 + 0700 是这里的卫生措施。
- **原生启动（Windows）：** 同样思路，用 PowerShell 自删除脚本，包含 `$env:KEY = '…'` 行（单引号转义）、窗口标题、颜色 env、`Set-Location`，最后执行 command block。
- **macOS GUI apps：** shell 不能把 env export 进 `.app`；脚本会把 `open …` 改写成逐个 `open --env KEY=VALUE …`，并用 osascript probe template 检查 app 是否已在运行（`macos_app_probe`）。
- **`cleanup_paths`：** plan 中列出的临时文件（例如 desktop 渲染的 overlay configs）会在命令退出后由脚本 `rm -f` 清掉，因此脚本不能 `exec`，要留下来做 cleanup。

## 各 OS 的终端处理

| OS | 机制 | 终端选择 |
|---|---|---|
| macOS | `open -b <bundle-id> <script.command>`（按 bundle id 解析，装在哪里都能找到） | `terminal`（Terminal.app，默认）、`iterm2`（检测 `/Applications` 与 `~/Applications` 下的 `iTerm.app` 或 `iTerm 2.app`）；其它值报错 |
| Windows | `open::with(script, "powershell.exe")`；app target 用 `Start-Process` 和 `windowsExecutablePath` 归一化；`windows_process_probe` 检查运行进程 | `powershell`（默认） |
| Linux | 通过候选列表启动脚本 | `system-terminal`（默认按顺序尝试：`xdg-terminal-exec`、`x-terminal-emulator`、`gnome-terminal --`、`konsole -e` 等）或显式 `gnome-terminal`、`konsole`、`xfce4-terminal`、`xterm`、`kitty`、`alacritty`、`wezterm` |

Preference 解析顺序：launch profile 里的显式选择 → 持久化 terminal config（`terminal_config.rs`，首次使用初始化）→ 平台默认值。不支持的组合（例如 macOS 上选 `konsole`）会验证失败，而不是静默 fallback。

## 参数处理

三个来源合并成最终命令行：

1. **Profile command args**（`RenderedProfile::command_args`）：必须是 args 而不是 env 的 provider routing。Codex 会得到 `-c model_providers.<id>.base_url=…` overrides；其它 agent 大多用 env。
2. **每个 agent 保存的 args**（`~/.vibearound/agents.json` prefs）：每个 agent 有两份列表：`launch_args.terminal`（原生启动）和 `launch_args.acp`（hosted spawn）。它们刻意分开：`--dangerously-skip-permissions` 可能适合你自己的终端，但不适合 IM 驱动的 host。
3. **Resume rendering**：agent registry 的 `resume_template`（`cd {cwd} && claude --resume {session_id}`）会变成 resume launch 的命令；没有 template 的 agent（kiro）不能原生 resume。

Launcher 会用 quote-aware parsing 把 `command` 字符串拆成 words（支持单/双引号、`\"` 转义），再追加 `args`，所以 registry 里的 `claude code --permission-mode acceptEdits` 这种命令能原样保留。

## 可执行文件解析（原生启动）

1. launch profile 里的 `executablePath`：验证后原样使用（Windows app launch 使用 `windowsExecutablePath` 变体）。
2. `~/.vibearound/agents.json` 里的 `agents.<agent>.executable.path`。
3. 对 command program 做 PATH 扫描；发现结果会**写回** `agents.json` 并在之后信任它。过期路径会验证失败，不会重新扫描（删除该条目可强制重新发现）。

App-launch wrappers（`open -a …`、`Start-Process …`）视为原生 app 命令，从不作为 CLI executable 缓存。

---

*Source anchors: `src/launcher/src/` (platform.rs — scripts and per-OS spawn, plan.rs — ExecutionPlan, executable.rs, terminal_config.rs, lib.rs — TerminalChoice), `src/core/src/agent/launch.rs` (materialize_profile_for_agent, profile-id env), `src/core/src/profiles/{render.rs,runtime.rs,bridge_launch.rs}` (env families, ConfigEnvTarget, profile-state dir), `src/core/src/agent_state.rs` (launch_args.terminal/acp), `src/desktop/src/profiles/launcher/` (desktop producer, resume plan), `src/core/src/process/env.rs` (hosted env).*
*Last verified: v0.7.11*

<sub>[◀ Module: server](modules/server.md) · [文档索引](../README.md)</sub>
