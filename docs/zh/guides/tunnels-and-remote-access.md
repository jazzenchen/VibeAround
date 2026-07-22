# 隧道与远程访问

隧道把你的控制台发布到一个公网 URL，让你离开机器也能访问 —— 地铁上的手机、咖啡馆里的笔记本。内置四家隧道供应商；每个远程浏览器进入前都必须配对。本页背后的信任规则见[安全模型](../architecture/security-model.md)。

## 选择供应商

| 供应商 | 设置值 | 需要账号 | 稳定主机名 |
|---|---|---|---|
| ngrok | `ngrok` | 需要（auth token） | 保留域名时有 |
| localtunnel | `localtunnel` | 不需要 | 无（每次启动随机） |
| Cloudflare Tunnel | `cloudflare` | 需要（tunnel token） | 有（你自己的主机名） |
| Tailscale Funnel | `tailscale` | 需要（已登录的 Tailscale 客户端） | 有（`*.ts.net`） |
| 禁用 | `none`（默认） | — | — |

经验法则：**localtunnel** 零配置试用；**ngrok** 最小配置获得个人稳定 URL；**Cloudflare** 在自己域名上使用永久主机名；主机已经在使用 Tailscale 时选 **Tailscale Funnel** 获得稳定公网 URL。

## 配置

在 [`~/.vibearound/settings.json`](../reference/configuration.md#settingsjson)（或桌面设置页面）：

```jsonc
{
  "tunnel": {
    "provider": "ngrok",
    "ngrok": {
      "auth_token": "2ab...",
      "domain": "myname.ngrok.app"          // 可选的保留域名
    }
  }
}
```

```jsonc
{
  "tunnel": {
    "provider": "cloudflare",
    "cloudflare": {
      "tunnel_token": "eyJ...",             // 来自 Zero Trust 控制台
      "hostname": "va.example.com"
    }
  }
}
```

```jsonc
{ "tunnel": { "provider": "localtunnel" } }
```

```jsonc
{ "tunnel": { "provider": "tailscale" } }
```

### Tailscale Funnel 需要已登录的客户端

安装 Tailscale、登录一个 tailnet，并启用 MagicDNS。VibeAround 会把 `tailscale funnel --yes http://127.0.0.1:12358` 作为前台子进程启动，读取公开的 `.ts.net` URL，并在 daemon 停止时同时关闭 Funnel。

第一次启动可能需要 owner 或 admin 在 Tailscale 网页控制台批准 Funnel。VibeAround 会显示“需要操作”和“启用 Funnel”按钮；只有你点击按钮时才会打开授权页面，批准后启动会继续，不需要运行终端命令。Funnel 是公网入口：远端浏览器不需要安装 Tailscale，VibeAround 配对仍然必需。tailnet 要求和平台限制见 [Tailscale Funnel](https://tailscale.com/docs/features/tailscale-funnel)。

### Cloudflare 需要一步手动操作

VibeAround 会启动 `cloudflared tunnel run --token …` 并用你的主机名生成 URL —— 但它**不会**创建 Cloudflare 的 *Published application route*。在 Zero Trust 里为同一条隧道添加一条：

| 字段 | 值 |
|---|---|
| Public hostname | VibeAround 里配置的主机名，如 `vibe.example.com` |
| Path | 留空（匹配所有路径） |
| Service | `HTTP` → `localhost:12358` |

然后打开 `https://vibe.example.com/va/` —— 控制台在 `/va/` 路径下，测试要用这个路径，不能只测根路径。

要把 Cloudflare 和 VibeAround 隔离排查：停掉 VibeAround，在 `127.0.0.1:12358` 上随便起个服务（`python3 -m http.server 12358 --bind 127.0.0.1`），再 `curl -i https://vibe.example.com/`。临时服务响应之前就出现 Cloudflare 404，说明问题在主机名/DNS/路由/隧道健康这一层，不在 VibeAround。参考：[Published applications](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/routing-to-tunnel/)、[run parameters](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/run-parameters/)。

隧道随守护进程启动。状态和公网 URL 显示在控制台、`va tunnels` 和桌面应用里；`va tunnel kill <provider>` 可以停掉某条隧道而不重启守护进程。

## 远程浏览器首次访问：配对

在新设备上打开公网 URL 会看到配对门：

1. 浏览器显示一个 6 位码（60 秒有效，可刷新）。
2. 在你已经信任的界面上确认它：
   - 在任意已连接的 IM 聊天里输入 `/pair <code>`，或
   - 在本机（控制台/桌面应用）批准，或
   - 用 CLI 的 `va pair` 流程（`pair start --wait --save` 还会保存认证，供 CLI 对远程守护进程使用）。
3. 浏览器绑定到守护进程当前的认证 token，之后表现得和本地浏览器一样。

配对在浏览器重启后仍有效，但守护进程重启后失效（token 会重新生成）。本地来源（`localhost`、`127.0.0.1`、桌面应用）永远不会看到配对门。

## 隧道暴露什么 —— 永远不暴露什么

配对之后，通过隧道可达：控制台 SPA、Web Chat、Web Terminal、预览和各 WebSocket 端点 —— 全部有 token 把守。预览的**分享链接**（`/preview/s/<slug>`）是唯一有意的例外：不用配对、不用 token、只开放单个预览、10 分钟过期。

永远不会通过隧道可达：本地 API Bridge 和 agent-as-API 端点（仅回环地址）、MCP 端点的本地 Bridge 面，以及任何形式的供应商凭据。

## 远程 CLI

`va` CLI 可以指向远程守护进程：`va --base-url https://va.example.com --token <token> status`；或通过配对流程保存一次认证（`va pair start --wait --save`），然后正常使用 `va`。`--auth-file` 指向另一份已保存的认证。

## 故障排查

| 症状 | 检查 |
|---|---|
| 公网 URL 一直不出现 | 供应商 token 无效或出口被拦 —— 守护进程日志有隧道错误；`va tunnels` 显示状态 |
| Cloudflare：隧道健康但 404 | 缺少或配错 Published application route —— 见上面的 Cloudflare 小节 |
| 配对码总是"invalid or expired" | 码只活 60 秒 —— 在窗口内生成并确认；确认输入的聊天连的是*同一个*守护进程 |
| 守护进程重启后全部 401 | 正常现象：token 重新生成了 —— 从受信入口重新打开并重新配对远程浏览器 |
| localtunnel URL 每次启动都变 | localtunnel 就是这样；要稳定用 ngrok 保留域名或 Cloudflare |
| Tailscale 显示“需要操作”但没有 URL | 点击“启用 Funnel”、完成 Tailscale 授权页面，并确认 Tailscale App 已登录 |
| Tailscale 在显示 URL 前退出 | 手动运行 `tailscale funnel http://127.0.0.1:12358`，确认当前客户端和平台支持 Funnel |
| 远程 Web Terminal 很卡 | 长链路隧道上的交互式 PTY 受延迟支配 —— 远程优先用 Web Chat，终端留在本地 |

---

*Source anchors: `src/core/src/tunnels/` (providers: ngrok, localtunnel, cloudflare, tailscale), `src/core/src/config.rs` (tunnel settings), `src/core/src/auth/pair.rs` (60 s codes), `src/server/src/web_server/auth.rs` (local-origin trust), `src/core/src/previews/store.rs` (share TTL), `src/cli/src/` (pair/tunnel commands).*
*Last verified: v0.7.19*

<sub>[◀ Agent 启动指南](agent-launch.md) · [文档索引](../README.md) · [开发渠道插件 ▶](build-a-channel-plugin.md)</sub>
