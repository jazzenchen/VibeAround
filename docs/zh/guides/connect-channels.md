# 连接渠道

在任何平台上，连接一个 IM 渠道都是三步：在平台侧创建 bot，把凭据放进 `settings.json` 的 `channels.<kind>` 下，然后让 VibeAround 启动插件。本页讲 VibeAround 这一侧和通用模式；平台侧的细节（bot 注册界面、权限 scope、webhook URL）在各插件自己的 README 里。

## 通用模式

1. **安装插件。** 桌面引导会把渠道插件装进 `~/.vibearound/plugins/<kind>/`；之后可用桌面插件管理器添加或更新。已安装但没有配置的插件保持禁用状态。

2. **配置渠道。** 在 `~/.vibearound/settings.json` 里添加一个 `channels.<kind>` 对象。键是插件相关的 —— VibeAround 把对象原样透传给插件。两个有代表性的例子：

```jsonc
{
  "channels": {
    "telegram": {
      "bot_token": "123456:ABC-DEF..."
    },
    "feishu": {
      "app_id": "cli_a1b2c3...",
      "app_secret": "..."
    }
  }
}
```

3. **启动。** 守护进程会在启动时拉起已配置的插件。运行中编辑了设置后，用下面的命令对齐：

```bash
va channel sync        # 启动新配置的，停止已移除的，重启有变更的
```

或使用桌面渠道控制。`va channels` 显示每个插件的运行状态。

## 按渠道选择默认值

`remote` 段为每种渠道指定默认 Agent 和 Profile，在聊天的第一条消息创建 Thread 时使用：

```jsonc
{
  "remote": {
    "channels": {
      "telegram": { "agent_id": "claude", "profile_id": "moonshot" },
      "feishu":   { "agent_id": "codex" }
    }
  }
}
```

用户仍可以在每个 Thread 里 `/switch`；这些只是初始值。

## 支持的渠道

每个渠道都有专门的配置页，含平台侧步骤和经代码验证的配置块：

| 渠道 | 配置页 |
|---|---|
| Telegram | [channels/telegram](channels/telegram.md) |
| 飞书 / Lark | [channels/feishu](channels/feishu.md) —— 权限 JSON、长连接订阅 |
| Slack | [channels/slack](channels/slack.md) —— 一键粘贴 app manifest、Socket Mode |
| Discord | [channels/discord](channels/discord.md) |
| 钉钉 | [channels/dingtalk](channels/dingtalk.md) |
| 企业微信 | [channels/wecom](channels/wecom.md) |
| QQ 机器人 | [channels/qqbot](channels/qqbot.md) |
| 微信 | [channels/wechat](channels/wechat.md) —— 扫码登录 |
| WhatsApp | [channels/whatsapp](channels/whatsapp.md) —— 扫码登录 |

Kind id 和仓库链接也在[支持矩阵](../product/supported-matrix.md#im-渠道)里。每个插件的 README 还额外记录：

- 如何创建 bot、获取凭据，
- 需要的平台权限/scope，
- 平台同时提供 webhook 和长轮询时如何选择，
- 平台能力与限制（卡片布局、附件大小、频率限制）。

## 健康与生命周期

插件受监督运行：崩溃会在短暂延迟后重新拉起；90 秒没有心跳的插件会被杀掉并重启。实际影响：

- 凭据配错通常表现为崩溃-重启循环 —— 查 `va channels` 和守护进程日志，改好配置后 `va channel restart <kind>`。
- 未送达的系统消息和权限卡片会排队，在插件重启后重发，所以插件抖动不会吃掉审批。
- 停止渠道（`va channel stop <kind>`）不会关闭它的 Thread；插件再启动时对话继续。

## 验证新渠道

1. `va channels` —— 插件显示为运行中。
2. 给 bot 发消息：应收到默认 Agent 的回复（首次联系会创建 Thread）。
3. 在聊天里 `/status` —— 确认 Thread、Workspace、Agent、Profile。
4. 触发一次权限（让 Agent 跑一条 shell 命令）—— 确认卡片正常渲染、点按后正常继续。

---

*Source anchors: `src/core/src/config.rs` (channel_names, channel raw config, RemoteConfig), `src/core/src/plugins/` (plugin directories), `src/core/src/channels/` (sync, supervision, live runtime routing), `src/cli/src/args.rs` (channel commands).*
*Last verified: v0.7.11*

<sub>[◀ IM 使用](im-usage.md) · [文档索引](../README.md) · [模型 Profile 指南 ▶](model-profiles.md)</sub>
