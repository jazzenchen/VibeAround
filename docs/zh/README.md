# VibeAround 文档

VibeAround 让你从已经在用的各种界面访问本地的 AI 编程 Agent（Claude Code、Codex、Gemini CLI 等）：Telegram、Slack、飞书等 IM 渠道，浏览器控制台，桌面控制应用，以及 TUI/CLI。一个本地运行时，一套 Workspace 模型，多扇进入它的门。

**第一次来？** 按这个顺序读：[VibeAround 是什么](product/what-is-vibearound.md) → [安装](guides/install-and-onboarding.md) → [快速导览](guides/quick-tour.md) → [核心概念](architecture/concepts.md)。其余内容按需查阅。

> 本目录是英文文档的中文版。中英内容不一致时，以[英文版](../README.md)为准。

## 分区

| 目录 | 内容 | 什么时候读 |
|---|---|---|
| [product/](#产品) | VibeAround 是什么、支持什么 | 你在评估它 |
| [guides/](#指南) | 面向任务的操作指南 | 你想把某件事做成 |
| [architecture/](#架构) | 核心概念与系统如何运转 | 你想理解它 |
| [reference/](#参考) | 速查表：设置、CLI、API 面、各种上限 | 你要核对一个细节 |
| [internals/](#internals) | 流程走读与各模块内幕 | 你在调试或修改代码 |

## 产品

| 页面 | 回答什么问题 |
|---|---|
| [VibeAround 是什么](product/what-is-vibearound.md) | 它解决什么问题？为谁解决？ |
| [支持矩阵](product/supported-matrix.md) | 支持哪些 Agent、渠道和模型供应商？ |

## 使用场景

面向场景的页面（同时也是网站的落地内容）：[远程编程](use-cases/remote-coding.md) · [手机上的 Codex](use-cases/codex-mobile.md) · [Codex 远程对比](use-cases/codex-remote.md) · [Claude Code 远程](use-cases/claude-remote.md) · [Gemini CLI 远程](use-cases/gemini-remote.md) · [OpenCode 远程](use-cases/opencode-remote.md) · [Claude Code 供应商切换](use-cases/claude-code-switcher.md)

## 指南

| 页面 | 帮你完成什么 |
|---|---|
| [安装与上手](guides/install-and-onboarding.md) | 安装桌面应用或 npm CLI，完成首次设置 |
| [下载](guides/download.md) | 当前桌面安装包、CLI 发行版和短下载路由 |
| [快速导览](guides/quick-tour.md) | 第一次聊天、第一个 IM 渠道、第一次会话交接 —— 15 分钟 |
| [CLI 快速开始](guides/cli-quick-start.md) | 安装 npm CLI，启动守护进程，跑通第一个终端工作流 |
| [桌面应用](guides/desktop-app.md) | 用 GUI 管理 Profile、启动和服务 |
| [Web 控制台](guides/web-dashboard.md) | Web Chat、实时预览 |
| [IM 使用](guides/im-usage.md) | 在聊天里驱动 Agent；完整斜杠命令参考 |
| [连接渠道](guides/connect-channels.md) | 配置 Telegram、Slack、飞书等渠道 |
| [模型 Profile](guides/model-profiles.md) | 供应商凭据与模型路由 |
| [宿主网页搜索](guides/web-search.md) | 配置宿主侧网页搜索兜底和供应商搜索替换 |
| [Agent 启动](guides/agent-launch.md) | 在你自己的终端里启动 Agent CLI |
| [隧道与远程访问](guides/tunnels-and-remote-access.md) | 在 localhost 之外访问控制台 |
| [开发渠道插件](guides/build-a-channel-plugin.md) | 用 SDK 为新的 IM 平台写插件 |
| [源码构建](guides/build-from-source.md) | 编译工作区并打包应用 |
| [故障排查与 FAQ](guides/troubleshooting-and-faq.md) | 解决常见问题 |

## 架构

| 页面 | 回答什么问题 |
|---|---|
| [核心概念](architecture/concepts.md) | Workspace、Thread、Route、Session、Agent、Profile 都是什么？ |
| [总览](architecture/overview.md) | 分层图、每一条通信边、模块地图 |
| [会话生命周期](architecture/session-lifecycle.md) | Thread 何时打开何时关闭？重启后什么会保留？ |
| [渠道插件系统](architecture/channel-plugin-system.md) | IM 集成在底层如何工作？ |
| [本地 API 与 Bridge](architecture/local-api-and-bridge.md) | 模型 API Bridge 如何在供应商之间转换？ |
| [安全模型](architecture/security-model.md) | 什么是受信的，什么要配对，什么走隧道？ |

## 参考

| 页面 | 内容 |
|---|---|
| [配置](reference/configuration.md) | settings.json、环境变量、数据目录 |
| [CLI](reference/cli.md) | 每一条 `va` 命令 |
| [API 面](reference/api-surfaces.md) | MCP 工具、本地 API 路由、WebSocket 端点、预览 URL |
| [计时器与上限](reference/timers-and-limits.md) | 每一个超时、TTL、间隔和大小限制 —— 唯一权威表 |
| [供应商端点](reference/provider-endpoints.md) | 各供应商的套餐、区域、base URL、模型与凭据语义 |

## Internals

用于调试和修改代码。完整地图见 [internals 索引](internals/README.md)。内容分成三类：

- **[architecture/](#架构)** 回答“为什么这样设计”，是读者层面的系统模型。
- **[internals/flows/](internals/README.md#flows)** 跟踪一条请求随时间经过的每一跳，并提供代码锚点。
- **[internals/modules/](internals/README.md#modules)** 描述一个组件在系统中的职责、关键类型、不变量和已知技术债。

要追踪行为，从 flow 开始；要修改组件，从 module 开始。Flow 和 module 会在交汇处互相链接。横切子系统有单独深挖，目前是 [Launch](internals/launch.md)。

## 本文档的约定

- `~/.vibearound/` 在所有平台上都是数据目录（可用 `VIBEAROUND_DATA_DIR` 覆盖）。
- 本地服务默认监听 `12358` 端口。
- Shell 示例使用 `va`，即 `npm i @vibearound/cli` 安装的 CLI。更长的别名 `vibearound` 在任何 `va` 可用的地方都可用。
- 每页结尾有 *Source anchors*（该页内容来源的代码文件）和 *Last verified* 版本号。改了被引用的文件，就更新对应页面并提升版本号。
- 页面之间用上一页/下一页链接串联，遵循推荐阅读顺序。

---

<sub>[English documentation](../README.md)</sub>
