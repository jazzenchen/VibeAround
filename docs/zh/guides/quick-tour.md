# 快速导览

十五分钟，三个"啊哈"时刻：在浏览器里和本地 Agent 聊天，在 IM 应用里做同样的事，然后把一个对话在两个界面之间移动。前提：已安装 VibeAround 并启用了至少一个 Agent（[安装与上手](install-and-onboarding.md)）。

## 1. 在浏览器里的第一次对话（3 分钟）

打开控制台（桌面应用托盘 → Dashboard，或 `va status` 显示的 URL），进入 **Web Chat**。

输入一件真实的事：

> look at ~/dev/my-app and summarize what this project does

观察发生了什么：

- 一个 Agent 进程在 Workspace 里被拉起 —— 第一条消息自动创建 Thread，无需任何设置。
- Agent 读文件、干活，输出实时流回来。
- 如果 Agent 想运行超出默认权限的命令，会出现**权限卡片**，就地批准或拒绝。

在聊天里试试 `/status`，看看你当前附着在什么上面：thread id、Workspace、宿主 Agent、Session。

## 2. 同一个 Agent，装进口袋（7 分钟）

连接一个 IM 渠道 —— Telegram 是最快的第一个：

1. 在 Telegram 侧创建 bot 并拿到 token —— 这一步发生在 Telegram，不在 VibeAround（[插件需要什么](channels/telegram.md)）。
2. 把凭据加进 VibeAround：
   - **桌面应用** —— 打开渠道页面，选 Telegram，粘贴 token。不用编辑任何文件。
   - **CLI / 无界面** —— 在 `settings.json` 的 `channels.telegram` 下添加，然后 `va channel sync`。（渠道配置的专用 CLI 命令在计划中；目前的方式是编辑文件。）
3. 和你的 bot 开个对话，打个招呼。

你的 bot 聊天现在就是一个完整的 Agent 对话：

```text
/new                        开一个全新 Thread
fix the failing test in ~/dev/my-app and show me the diff
```

权限请求会以可点按的按钮送达。这个聊天拥有**自己的** Thread —— 与你的 Web Chat 互相独立，两边可以同时进行。

## 3. 一个对话，两个界面（5 分钟）

连续性才是重点，所以来移动点什么：

**终端 → 手机。** 通过 VibeAround 启动一个 Agent CLI（桌面 **Launch**，或 `va launch --profile <name>`），干一会儿活，然后让 Agent 运行它的交接工具（`vibearound` 技能把它暴露为 `/vibearound handover`）。你会得到一个 4 位短码，两分钟内有效。在 Telegram bot 聊天里：

```text
/pickup K7PQ
```

聊天就附着到了那个终端会话上 —— 同样的上下文、同样的 Workspace，从离开的地方继续。

**在两个地方同时看。** 在 IM 回合进行时打开 Web Chat：Thread 会把输出扇出到每一个附着的界面，同一个回合在两边同时流式呈现。

## 接下来去哪

| 你想…… | 读 |
|---|---|
| 理解刚才发生了什么（Thread、Route、Session） | [核心概念](../architecture/concepts.md) |
| 一份供应商订阅供多个 Agent CLI 使用 | [模型 Profile 指南](model-profiles.md) |
| 出门在外访问控制台 | [隧道与远程访问](tunnels-and-remote-access.md) |
| 完整的斜杠命令列表 | [IM 使用](im-usage.md) |
| 在本机预览 dev server，或用可重复使用的六位访问码把 Markdown 分享 10 分钟 | [Web 控制台指南](web-dashboard.md) |

---

*Source anchors: `src/core/src/channels/prompt/handler.rs` (commands), `src/core/src/workspace/handover.rs` (pickup), `src/skills/vibearound/SKILL.md` (handover skill).*
*Last verified: v0.7.11*

<sub>[◀ 安装与上手](install-and-onboarding.md) · [文档索引](../README.md) · [桌面应用指南 ▶](desktop-app.md)</sub>
