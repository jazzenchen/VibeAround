# 安全模型

VibeAround 暴露的都是强能力 —— 终端、有 shell 权限的编程 Agent、你的供应商凭据 —— 所以值得精确理解它的信任边界。简短版本：一切都在你的机器上运行，localhost 经 token 认证后受信，从外部进来的任何东西都必须配对，凭据永远不离开守护进程。

## 信任区

```text
zone 0  守护进程            持有凭据，拉起 Agent
zone 1  localhost 客户端    桌面应用、本地浏览器、va CLI —— token 认证
zone 2  隧道化浏览器        需要配对码 + token
zone 3  IM 平台             只有消息，经插件和权限卡片中介
```

## Zone 1：本地客户端与认证 token

守护进程每次启动生成一个全新的随机 bearer token，写入 [`~/.vibearound/auth.json`](../reference/configuration.md#all-files-on-disk)。所有受保护的 HTTP 和 WebSocket 路由都需要它，形式是 `Authorization: Bearer <token>` 或 `?token=` 查询参数。桌面应用打开控制台时附带 token；`va` CLI 读 token 文件；守护进程重启后拿旧 token 的浏览器会被拒绝，必须从授权入口重新加载。

实际后果：

- 另一个本地用户（或恶意软件）不能仅因端口开着就驱动你的 Agent —— 它需要 token 文件，而那在你的家目录里。
- 重启守护进程会让之前签发的所有 URL 失效。

## Zone 2：经隧道的远程访问

隧道（ngrok、localtunnel、Cloudflare、Tailscale Funnel）把控制台发布到公网 URL。两道门：

1. **配对。** 非本地主机名上的浏览器必须完成配对：控制台显示一个 60 秒过期的 6 位码，必须在已受信的界面上确认 —— 输入到已连接的 IM 聊天（`/pair <code>`）或在本机批准。配对把该浏览器绑定到守护进程当前的认证 token（全部 TTL 见[计时器与上限](../reference/timers-and-limits.md#lifecycles-and-ttls)）。
2. **Token。** 配对之后，与本地相同的 bearer token 规则适用。

本地主机名（`localhost`、`127.0.0.1`、`::1` 和桌面应用自己的 origin）跳过配对，但永远不跳过 token。

## Zone 3：IM 平台

IM 消息经渠道插件到达。平台永远拿不到 shell，它拿到的是一段对话：

- **权限卡片。** Agent 想运行命令或在允许范围外编辑时，权限请求渲染为聊天里的交互卡片。有人点按选择之前，Agent 的回合一直阻塞。插件在请求中途死掉，请求会被取消而不是被默默批准。
- **Route 隔离。** 每个聊天映射到自己的 Thread 和 Workspace；一个群聊不能看到或操纵另一个聊天的 Agent。
- **附件卫生。** 来自插件的 file key 在成为文件引用前会做路径穿越校验（`..`、分隔符）；不安全的 key 被丢弃。
- **Bot 凭据**（平台 token）存在你机器上的 `settings.json` 里，只传给拥有它的那个插件进程。

## 预览：本地 Server、受限 Markdown 分享

Live Server 与 Markdown 渲染预览采用不同边界：

| 目标与 URL | 受众 | 寿命 |
|---|---|---|
| Server `/preview/u/<slug>` | 仅回环浏览器 | 预览存在期间 |
| Markdown `/preview/u/<slug>` | 回环浏览器或已配对 owner | 预览存在期间 |
| Markdown `/preview/s/<share_id>` | 持有 Share URL 和六位访问码的人 | URL、访问码和浏览器授信共用 600 秒期限 |

Live Server 预览不生成分享链接，也不能通过公网 hostname 加载。在 loopback 上，owner shell 直接 iframe 准确的 dev-server origin；它不会转发 owner bearer token，也不代理应用的 Fetch、WebSocket 或 HMR 流量。不同端口把子页面与 owner 的 DOM、storage 隔离开，但 dev server 自己的 iframe 策略和浏览器能力仍然生效。Markdown 分享不使用 owner 配对：不透明 URL 先显示访问码门，验证成功后签发 `Secure`、`HttpOnly`、按路径限定的浏览器授信。URL、可重复使用的六位访问码和授信只对应一个文档，并在 10 分钟后同时过期。隧道上的其他一切仍需配对 + token。

Markdown 解析器和净化器都随守护进程内置，Preview 不执行远程解析脚本。GitHub 风格的原始 HTML 只有通过安全白名单后才会渲染，脚本、样式、iframe、表单、事件处理器和未支持的属性都会被移除。Markdown 图片语法和允许的原始 HTML 图片都必须使用绝对 HTTPS URL。图片主机可以看到访问者的 IP 地址；`Referrer-Policy: no-referrer` 会阻止 Preview URL 随请求发送。

## 凭据处理

- 供应商 API key 存在 `~/.vibearound/` 下的 Profile 存储里，**由守护进程**注入上游请求。渲染给 Agent 的配置只含本地 Bridge URL —— Agent 配置泄露不会暴露任何供应商 key。
- Bridge 和 agent-as-API 端点仅监听回环地址，且有本地 bridge 门；隧道无法触达。主路径 `/local-api` 和 `/local-agent` 各有独立、随守护进程轮换的 scoped token，因此模型 Bridge 客户端不能启动 Agent。
- 守护进程自己的 token 文件是家目录里的明文（信任级别等同 `~/.ssh`）；备份时相应对待。
- 启动弹窗的 Bridge 请求/响应记录只在内存里，从不落盘。

## VibeAround 不防什么

模型的诚实边界：

- **恶意的本地 root/用户账号。** 能读你家目录的人就有你的 token 和凭据。
- **已授权限内的恶意 Agent 行为。** 权限卡片把守的是提权，但你批准的命令以你用户的完整权限运行。
- **IM 平台被攻破。** 有人控制了你的 Telegram 账号，就能以你的身份和 Agent 对话 —— 剩下的屏障只有权限卡片。敏感工作优先用按聊天隔离的 bot 和私聊。

---

*Source anchors: `src/server/src/web_server/auth.rs` (token middleware, local-origin rules), `src/core/src/auth/` (token file, pairing TTL), `src/core/src/previews/store.rs` (SHARE_TTL_SECS), `src/core/src/routing.rs` (attachment key validation), `src/server/src/web_server/mod.rs` (route protection layout).*
*Last verified: v0.7.11*

<sub>[◀ 本地 API 与 Bridge](local-api-and-bridge.md) · [文档索引](../README.md) · [配置参考 ▶](../reference/configuration.md)</sub>
