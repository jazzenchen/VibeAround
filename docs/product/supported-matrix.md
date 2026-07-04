# Supported matrix

Which coding agents, IM channels, and model providers VibeAround supports, and what each combination can do. Ids in the tables are the values you use in `settings.json`, slash commands, and CLI flags.

## Coding agents

VibeAround drives agents over the Agent Client Protocol (ACP). Every hosted agent supports the full conversation flow: prompting, streaming output, permission requests, and cancellation.

| Agent id | Product | Hosted (IM / web chat) | Session resume | Notes / aliases |
|---|---|---|---|---|
| `claude` | Claude Code | Yes | Yes | `claude-code` |
| `codex` | Codex CLI | Yes | Yes | `openai-codex` |
| `gemini` | Gemini CLI | Yes | Yes | `gemini-cli` |
| `cursor` | Cursor CLI | Yes | Yes | — |
| `qwen-code` | Qwen Code | Yes | Yes | `qwen` |
| `opencode` | Opencode | Yes | Yes | `open-code` |
| `pi` | Pi | Yes | Yes | `pi-agent`, `pi-coding-agent` |
| `kiro` | Kiro CLI | Yes | No resume template yet | `kiro-cli` |
| `claude-desktop` | Claude Desktop | **Launch target only** | — | Opens the desktop app; not an ACP runtime |
| `codex-desktop` | Codex Desktop | **Launch target only** | — | Opens the desktop app; not an ACP runtime |

"Session resume" means VibeAround can restore the agent's own CLI session across idle shutdowns, daemon restarts, and handovers.

## IM channels

Channels are plugins (separate npm packages); the web chat and TUI are built-in surfaces that use the same routing. All channels support the core flow: prompts, streamed replies, slash commands, and permission cards.

| Channel | Kind id | Delivered as | Repository |
|---|---|---|---|
| Telegram | `telegram` | Plugin | [va-plugin-channel-telegram](https://github.com/jazzenchen/va-plugin-channel-telegram) |
| Slack | `slack` | Plugin | [va-plugin-channel-slack](https://github.com/jazzenchen/va-plugin-channel-slack) |
| Feishu / Lark | `feishu` | Plugin (interactive cards use the V2 card schema) | [va-plugin-channel-feishu](https://github.com/jazzenchen/va-plugin-channel-feishu) |
| Discord | `discord` | Plugin | [va-plugin-channel-discord](https://github.com/jazzenchen/va-plugin-channel-discord) |
| WeChat | `weixin-openclaw-bridge` | Plugin | [va-plugin-channel-weixin-openclaw-bridge](https://github.com/jazzenchen/va-plugin-channel-weixin-openclaw-bridge) |
| WhatsApp | `whatsapp` | Plugin | [va-plugin-channel-whatsapp](https://github.com/jazzenchen/va-plugin-channel-whatsapp) |
| DingTalk | `dingtalk` | Plugin | [va-plugin-channel-dingtalk](https://github.com/jazzenchen/va-plugin-channel-dingtalk) |
| WeCom (企业微信) | `wecom` | Plugin | [va-plugin-channel-wecom](https://github.com/jazzenchen/va-plugin-channel-wecom) |
| QQ Bot | `qqbot` | Plugin | [va-plugin-channel-qqbot](https://github.com/jazzenchen/va-plugin-channel-qqbot) |
| Web chat | `web` | Built-in (dashboard) | — |
| TUI chat | `tui` | Built-in (`vibearound tui`) | — |

Per-platform capabilities (attachments, card layouts, group behavior) are documented in each plugin's own README; configuration lives under `channels.<kind>` in `settings.json` — see [Connect channels](../guides/connect-channels.md).

## Model providers (profile catalog)

The built-in catalog ships endpoint and model definitions for these providers. Each endpoint declares which API dialects it serves — the bridge translates whenever your agent's dialect differs (see [Local API and bridge](../architecture/local-api-and-bridge.md)).

| Provider id | Label | Upstream dialects available | Notes |
|---|---|---|---|
| `moonshot` | Kimi / Moonshot | openai-chat, anthropic | Global + CN endpoints, Kimi coding plan |
| `dashscope` | Alibaba DashScope | anthropic, openai-responses, openai-chat | CN endpoints, coding/token plans |
| `deepseek` | DeepSeek | anthropic, openai-chat | Dedicated translation adapter |
| `openrouter` | OpenRouter | openai-chat | — |
| `minimax` | MiniMax | anthropic, openai-chat, openai-responses | Global + CN, token plans |
| `mimo` | Xiaomi MiMo | openai-chat, anthropic | Pay-as-you-go + CN token plan |
| `volcengine` | Volcengine | anthropic, openai-responses, openai-chat | Ark API, coding/agent plans |
| `zai` | Z.AI / GLM | openai-chat, anthropic | Global + CN, coding plans |
| `gemini` | Google Gemini / Vertex AI | gemini, openai-chat | Gemini API + Vertex-compatible |
| `xai` | xAI / Grok | openai-responses, openai-chat | — |
| `nvidia` | NVIDIA NIM | openai-chat | — |
| `azure` | Azure OpenAI | openai-responses | Custom deployment fields |

A **custom** profile (any OpenAI/Anthropic/Gemini-compatible endpoint) is always available beyond the catalog, and the `direct` profile launches an agent with its own vendor login and no bridge.

Plan-by-plan detail — endpoint groups, regions, base URLs, model sets, and which credential each plan expects — is in the [provider endpoints reference](../reference/provider-endpoints.md).

## Platforms

| Platform | Desktop app | Standalone server / CLI |
|---|---|---|
| macOS Apple Silicon | Packaged (DMG) | Yes |
| macOS Intel | Source build | Yes |
| Windows x64 | Packaged (EXE/MSI/portable) | Yes |
| Linux x64 | Packaged (AppImage/deb); terminal launch depends on desktop environment | Yes |

---

*Source anchors: `src/resources/agents.json` (agent registry — ids, aliases, direct_only, resume templates), `src/resources/plugins.json` (channel plugin registry — kind ids and repositories), `src/resources/profile-catalog/` (provider endpoints and dialects), `src/server/src/lib.rs` (built-in web/tui channels), `README.md` (packaging status).*
*Last verified: v0.7.11*

<sub>[◀ What is VibeAround](what-is-vibearound.md) · [Documentation index](../README.md) · [Install and onboarding ▶](../guides/install-and-onboarding.md)</sub>
