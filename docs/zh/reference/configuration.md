# 配置参考

VibeAround 读写的每一个文件，可手动编辑的都附完整 schema。姐妹页：[CLI 参考](cli.md)、[API 面](api-surfaces.md)、[供应商端点](provider-endpoints.md)。

## 磁盘上的所有文件

一切都在 `~/.vibearound/` 下（可用 `VIBEAROUND_DATA_DIR` 覆盖）：

| 文件 / 目录 | 由谁写 | 内容 | 可手动编辑？ |
|---|---|---|---|
| `settings.json` | 你、桌面设置 UI、引导 | 主配置 —— [完整 schema 见下](#settingsjson) | **可以**（改后 `va settings reload`） |
| `agents.json` | 桌面 Launch UI、`va-launch`（可执行文件发现） | 按 Agent 的启动偏好 —— [schema 见下](#agentsjson) | 可以，小心改 |
| `launch/profiles/<name>.json` | 你、桌面（临时物化副本） | 保存的原生启动配置 —— [schema 见下](#launch-profile-json-schema-v1) | **可以**（本来就是给你编辑的） |
| `auth.json` | 守护进程，每次启动 | 供进程外客户端使用的 `{port, token}` | 不行 —— 每次启动重写 |
| `profiles/<profile-id>.json` | 桌面/控制台 Profile UI | 保存的模型 Profile（供应商、端点、key、模型路由） | 优先用 UI；手动改动会在重载时读取 |
| `profile-state/<profile-id>/` | Profile 渲染 | 渲染出的按 Profile 的 Agent 配置文件（设置覆盖层）；环境指针引用这些（[启动内幕](../internals/launch.md#environment-assembly-layer-by-layer)） | 不行 —— 每次渲染重新生成 |
| `plugins/<kind>/` | 桌面插件管理器 | 已安装的渠道插件 + 清单 | 仅插件开发期间 |
| `workspaces/` | 守护进程 | 新建 Workspace 的默认根 | 那是你自己的文件 |
| `.cache/` | 渠道插件 | 下载的聊天附件 | 可安全清空 |
| `logs/runtime/` | 守护进程 | 按日滚动日志（`vibearound.log.<date>`） | 可安全清空 |
| `*.jsonl`（workspace/thread/attachment 事件日志） | 守护进程 | 对话状态（[workspace 模块](../internals/modules/workspace.md)） | **不行** —— 只追加的事件日志 |
| `launcher.json` | 桌面 | 桌面专属启动偏好（终端选择、按 Agent 的 workspace 兼容性） | 优先用 UI |
| `desktop-apps.detected.json` | 桌面检测 | 缓存的 Claude/Codex Desktop 应用位置 | 不行 —— 缓存 |

VibeAround 还会写**每个已启用 Agent 自己的全局配置**（MCP server 条目 + 技能文件，路径由 Agent 注册表声明 —— 例如 `~/.claude.json` 的 `mcpServers` 和 `~/.claude/skills/vibearound/`）。这些写入带 VibeAround 管理标记，守护进程停机时会被启动期清理移除（[启动流程第 5 步](../internals/flows/native-launch.md)）。

## settings.json

位置：`~/.vibearound/settings.json`。首次运行以默认值创建；用 `va settings reload`、桌面重载操作或重启守护进程使编辑生效。未知键会被忽略。

```jsonc
{
  // --- 隧道（见 ../guides/tunnels-and-remote-access.md） ---
  "tunnel": {
    "provider": "none",              // none | ngrok | localtunnel | cloudflare
    "ngrok":      { "auth_token": "…", "domain": "…" },
    "cloudflare": { "tunnel_token": "…", "hostname": "…" }
  },
  "preview_base_url": null,          // 覆盖预览链接的公开 base URL

  // --- 工具链 ---
  "toolchain_mode": "system",        // system | managed

  // --- Workspace ---
  "default_workspace": "~/…",        // 新 Agent 会话的根目录
  "workspaces": ["~/dev/app-a"],     // 额外注册的项目目录

  // --- Agent ---
  "default_agent": "claude",
  "enabled_agents": ["claude", "codex"],  // 省略则启用所有已知 Agent
  "integrations": {
    "mcp_auto_install": true,        // 把 VibeAround MCP 配置写进 Agent 配置
    "skill_auto_install": true      // 把 VibeAround 技能写进 Agent 技能目录
  },

  // --- 网络 ---
  "proxy": { "enabled": true, "http_proxy": "http://…", "no_proxy": "…" },

  // --- Bridge 行为（见 ../architecture/local-api-and-bridge.md） ---
  "api_bridge": {
    "replace_provider_web_search": false
  },
  "local_agent_api": { "enabled": true },

  // --- 宿主侧网页搜索 ---
  "search_tool": {
    "enabled": false,
    "max_results": 5,
    "sources": {
      "tavily": { "enabled": true, "api_key": "…" },   // 还有：brave、exa、grok
      "brave":  { "enabled": false, "api_key_env": "BRAVE_KEY", "base_url": null }
    }
  },

  // --- 按渠道默认值（见 ../guides/connect-channels.md） ---
  "remote": {
    "channels": { "telegram": { "agent_id": "claude", "profile_id": "moonshot" } }
  },

  // --- 渠道插件配置：原样透传给插件 ---
  // 各渠道字段见 guides/channels/。每个渠道都接受可选的
  // verbose 对象（两个开关默认都是 false）。
  "channels": {
    "telegram": { "bot_token": "…", "verbose": { "show_thinking": true, "show_tool_use": true } },
    "feishu":   { "app_id": "…", "app_secret": "…" }
  },

  // --- Web 终端 ---
  "tmux": { "detach_others": true }
}
```

## agents.json

三层结构的启动偏好。Agent id 接受注册表别名（[支持矩阵](../product/supported-matrix.md)）。

```jsonc
{
  "selected_agent": "claude",        // Launch 页当前显示的 Agent（UI 状态）
  "default_agent": "claude",         // VibeAround 全局默认：托盘快速启动、IM Thread 创建
  "default_profile_id": "moonshot",  // 该默认值的 Profile 快照
  "agents": {
    "codex": {
      "profile_id": "deepseek",      // 按 Agent 的默认 Profile
      "workspace": "~/dev/app",      // 按 Agent 的默认 Workspace
      "executable": {                 // 解析出的 CLI —— 由 va-launch 发现后写回
        "path": "/opt/homebrew/bin/codex",
        "version": "…", "source": "path-scan", "rank": 0
      },
      "launch_args": {
        "terminal": ["--flag-for-your-own-terminal"],  // 仅原生启动
        "acp": ["--flag-for-hosted-spawns"]            // 仅 IM/web 托管拉起
      }
    }
  }
}
```

两个 `launch_args` 列表刻意分开 —— 你在自己终端里信任的参数，对 IM 驱动的托管进程不自动安全（[启动内幕](../internals/launch.md#argument-handling)）。过期的 `executable.path` 会让启动校验失败；删掉该条目可强制重新扫描 PATH。

## 启动配置 JSON（schema v1）

保存在 `launch/profiles/<name>.json`，由 `va launch --profile <name>` / `--profile-path <file>` 消费。**未知字段会被拒绝** —— 把供应商 Profile 或其他 JSON 递给启动器会大声报错，而不是半残地工作。

```jsonc
{
  "schemaVersion": 1,
  "id": "openai-codex",              // 配置名
  "agent": "codex",                  // 注册表 Agent id
  "profileId": "openai",             // 仅元数据 —— va-launch 从不读供应商存储
  "launchTarget": "codex",
  "workspace": "/Users/example/project",
  "terminal": "terminal",            // 终端 id；按 OS 的列表见启动内幕
  "command": "codex",                // 命令行（按引号感知分词）
  "executablePath": null,            // 显式 CLI 覆盖（跳过 agents.json + PATH）
  "windowsExecutablePath": null,     // Windows 应用启动变体
  "windowLabel": "OpenAI Codex",
  "env": { "OPENAI_API_KEY": "…" },  // 由生成的启动脚本导出
  "args": { "native": ["--model", "gpt-5"] },
  "cleanupPaths": [],                // 命令退出后删除的临时文件
  "macosAppProbe": null,             // "已在运行"osascript 检查用的应用名
  "windowsProcessProbe": null
}
```

两个"profile"概念在此相遇，不可混淆：**供应商 Profile**（凭据 + 模型路由，在应用里管理）vs **启动 Profile**（这个文件 —— 一次原生启动请求）。一个解析器连接两者：桌面在启动时把供应商 Profile *渲染成*物化的启动 Profile；保存的 CLI 启动配置持有渲染后的快照（[启动内幕](../internals/launch.md#producers-desktop-vs-cli)）。

## 环境变量

| 变量 | 消费者 | 含义 |
|---|---|---|
| `VIBEAROUND_DATA_DIR` | 守护进程、va-launch | 覆盖 `~/.vibearound` |
| `RUST_LOG` | 守护进程 | 日志过滤（默认 `info,common=debug`）；见[故障排查](../guides/troubleshooting-and-faq.md#日志在哪) |
| `VIBEAROUND_VA_LAUNCH_BIN` | 桌面/CLI（仅开发） | 指向未打包的 `va-launch` |
| `VIBEAROUND_CHANNEL_KIND`、`VIBEAROUND_CHAT_ID`、`VIBEAROUND_AGENT_KIND`、`VIBEAROUND_THREAD_ID`、`VIBEAROUND_WORKSPACE_ID` | 托管的 Agent 进程 | 注入的所属 Route/Thread 上下文 |

## 数据目录

```text
~/.vibearound/
├── settings.json           # 配置（本页）
├── auth.json               # 控制台 token，每次守护进程启动重写
├── agents.json             # 解析出的 Agent 可执行文件（va-launch 缓存）
├── plugins/<kind>/         # 已安装的渠道插件
├── launch/profiles/        # 保存的启动配置 JSON（schema v1）
├── workspaces/             # 新建 Workspace 的默认根
├── .cache/                 # 渠道附件缓存
└── workspace-threads.jsonl # + workspace/attachment 事件日志
```

默认端口：`12358`。控制台：`http://127.0.0.1:12358/va/`（需要 token；根路径重定向到 `/va/`）。

---

*Source anchors: `src/core/src/config.rs` (settings parser — key names above mirror it), `src/core/src/workspace/threads/runtime.rs` (injected env), `src/launcher/` (agents.json, launch profiles).*
*Last verified: v0.7.11*

<sub>[◀ 安全模型](../architecture/security-model.md) · [文档索引](../README.md) · [CLI 参考 ▶](cli.md)</sub>
