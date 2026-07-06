# 支持矩阵

VibeAround 支持哪些编程 Agent、IM 渠道和模型供应商，以及每种组合能做什么。表中的 id 就是你在 `settings.json`、斜杠命令和 CLI 参数里使用的值。

## 编程 Agent

VibeAround 通过 Agent Client Protocol（ACP）驱动 Agent。每个托管 Agent 都支持完整的对话流程：提示、流式输出、权限请求和取消。

| Agent id | 产品 | 托管（IM / Web Chat） | 会话恢复 | 备注 / 别名 |
|---|---|---|---|---|
| `claude` | Claude Code | 支持 | 支持 | `claude-code` |
| `codex` | Codex CLI | 支持 | 支持 | `openai-codex` |
| `gemini` | Gemini CLI | 支持 | 支持 | `gemini-cli` |
| `cursor` | Cursor CLI | 支持 | 支持 | — |
| `qwen-code` | Qwen Code | 支持 | 支持 | `qwen` |
| `opencode` | Opencode | 支持 | 支持 | `open-code` |
| `pi` | Pi | 支持 | 支持 | `pi-agent`, `pi-coding-agent` |
| `kiro` | Kiro CLI | 支持 | 暂无恢复模板 | `kiro-cli` |
| `claude-desktop` | Claude Desktop | **仅作为启动目标** | — | 打开桌面应用；不是 ACP 运行时 |
| `codex-desktop` | Codex Desktop | **仅作为启动目标** | — | 打开桌面应用；不是 ACP 运行时 |

"会话恢复"指 VibeAround 能在闲时关停、守护进程重启和会话交接之后，恢复 Agent 自己的 CLI 会话。

## IM 渠道

渠道以插件形式提供（独立的 npm 包）；Web Chat 和 TUI 是使用同一套路由的内置界面。所有渠道都支持核心流程：提示、流式回复、斜杠命令和权限卡片。

| 渠道 | Kind id | 交付形式 | 仓库 |
|---|---|---|---|
| Telegram | `telegram` | 插件 | [va-plugin-channel-telegram](https://github.com/jazzenchen/va-plugin-channel-telegram) |
| Slack | `slack` | 插件 | [va-plugin-channel-slack](https://github.com/jazzenchen/va-plugin-channel-slack) |
| 飞书 / Lark | `feishu` | 插件（交互卡片使用 V2 卡片 schema） | [va-plugin-channel-feishu](https://github.com/jazzenchen/va-plugin-channel-feishu) |
| Discord | `discord` | 插件 | [va-plugin-channel-discord](https://github.com/jazzenchen/va-plugin-channel-discord) |
| 微信 | `weixin-openclaw-bridge` | 插件 | [va-plugin-channel-weixin-openclaw-bridge](https://github.com/jazzenchen/va-plugin-channel-weixin-openclaw-bridge) |
| WhatsApp | `whatsapp` | 插件 | [va-plugin-channel-whatsapp](https://github.com/jazzenchen/va-plugin-channel-whatsapp) |
| 钉钉 | `dingtalk` | 插件 | [va-plugin-channel-dingtalk](https://github.com/jazzenchen/va-plugin-channel-dingtalk) |
| 企业微信 | `wecom` | 插件 | [va-plugin-channel-wecom](https://github.com/jazzenchen/va-plugin-channel-wecom) |
| QQ 机器人 | `qqbot` | 插件 | [va-plugin-channel-qqbot](https://github.com/jazzenchen/va-plugin-channel-qqbot) |
| Web Chat | `web` | 内置（控制台） | — |
| TUI 聊天 | `tui` | 内置（`vibearound tui`） | — |

各渠道的配置页（平台侧步骤 + 验证过的配置块）在 [guides/channels/](../guides/connect-channels.md#supported-channels)；配置写在 `settings.json` 的 `channels.<kind>` 下。

## 模型供应商（Profile 目录）

内置目录为以下供应商预置了端点和模型定义。每个端点声明自己服务哪些 API 方言 —— 只要 Agent 的方言和端点不同，Bridge 就做转换（见[本地 API 与 Bridge](../architecture/local-api-and-bridge.md)）。

| Provider id | 名称 | 可用上游方言 | 备注 |
|---|---|---|---|
| `moonshot` | Kimi / Moonshot | openai-chat, anthropic | 全球 + 国内端点，Kimi 编程套餐 |
| `dashscope` | 阿里云百炼 DashScope | anthropic, openai-responses, openai-chat | 国内端点，编程/token 套餐 |
| `deepseek` | DeepSeek | anthropic, openai-chat | 专用转换适配器 |
| `openrouter` | OpenRouter | openai-chat | — |
| `minimax` | MiniMax | anthropic, openai-chat, openai-responses | 全球 + 国内，token 套餐 |
| `mimo` | 小米 MiMo | openai-chat, anthropic | 按量付费 + 国内 token 套餐 |
| `volcengine` | 火山引擎 | anthropic, openai-responses, openai-chat | Ark API，编程/Agent 套餐 |
| `zai` | Z.AI / GLM | openai-chat, anthropic | 全球 + 国内，编程套餐 |
| `gemini` | Google Gemini / Vertex AI | gemini, openai-chat | Gemini API + Vertex 兼容 |
| `xai` | xAI / Grok | openai-responses, openai-chat | — |
| `nvidia` | NVIDIA NIM | openai-chat | — |
| `azure` | Azure OpenAI | openai-responses | 自定义部署字段 |

目录之外始终可以创建 **custom** Profile（任何 OpenAI/Anthropic/Gemini 兼容端点）；`direct` Profile 则让 Agent 用自己的官方登录启动，不经过 Bridge。

按套餐细分的信息 —— 端点分组、区域、base URL、模型集合，以及每种套餐需要哪种凭据 —— 见[供应商端点参考](../reference/provider-endpoints.md)。

## 平台

| 平台 | 桌面应用 | 独立服务端 / CLI |
|---|---|---|
| macOS Apple Silicon | 已打包（DMG） | 支持 |
| macOS Intel | 源码构建 | 支持 |
| Windows x64 | 已打包（EXE/MSI/便携版） | 支持 |
| Linux x64 | 已打包（AppImage/deb）；终端启动依赖桌面环境 | 支持 |

---

*Source anchors: `src/resources/agents.json` (agent registry — ids, aliases, direct_only, resume templates), `src/resources/plugins.json` (channel plugin registry — kind ids and repositories), `src/resources/profile-catalog/` (provider endpoints and dialects), `src/server/src/lib.rs` (built-in web/tui channels), `README.md` (packaging status).*
*Last verified: v0.7.11*

<sub>[◀ VibeAround 是什么](what-is-vibearound.md) · [文档索引](../README.md) · [安装与上手 ▶](../guides/install-and-onboarding.md)</sub>
