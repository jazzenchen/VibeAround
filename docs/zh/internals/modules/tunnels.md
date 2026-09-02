# Module: tunnels

`src/core/src/tunnels/`（当前位置）：通过四个可互换 provider，把 server 的 web listener 发布到 public URL。

## 职责

启动、跟踪、停止 tunnel runtimes；向系统其它部分暴露当前 public URL。Tunnel 在语义上是 **server exposure capability**：目标是实际 bound web listener，安全策略与 server auth/origin 强耦合；provider mechanics 可留在下层复用。

## 关键类型

| Type | File | Role |
|---|---|---|
| `TunnelManager` | `mod.rs` | Live tunnels registry：provider → URL、registry id、abort handle；实现 `StateSource` |
| `start_web_tunnel_with_provider` | `mod.rs` | 入口：config → provider start → (guard, public URL) |
| ngrok provider | `providers/ngrok.rs` | spawn ngrok agent（`ngrok http`），URL 从其 JSON 日志解析；可选 reserved domain |
| cloudflare provider | `providers/cloudflare.rs` | 子进程：`cloudflared tunnel run --token …` |
| localtunnel provider | `providers/localtunnel.rs` | 子进程：当前硬编码 `npx localtunnel --port 12358`（已知 target-port 缺陷） |
| tailscale provider | `providers/tailscale.rs` | 子进程：`tailscale funnel --yes http://127.0.0.1:12358` |

## 交互

- **← server (daemon boot)：** 启动配置的 tunnel，上报 Tailscale 的 `awaiting_approval` 状态，注册 abort handle；`stop()` abort 并清空。
- **← auth：** 存在 public hostname 时触发 pairing gate。
- **← previews / dashboard：** 当前运行中的 tunnel URL 用于已配对的 Server/Markdown owner 链接和由访问码保护的 Share 链接。
- **→ resources：** provider program definitions 和 spawn-error hints（例如“is Node/npx installed?”）。

## 不变量：不要破坏

1. **`none` 是一等 provider**：不跑 tunnel code，不 spawn child；新 call sites 必须容忍 URL 缺失。
2. **Tunnel 只暴露 web listener**：不要通过它绑定额外端口；loopback-only surfaces（local-api）必须保持不可达。
3. **Server Share 代理必须保持页面导向**：原样转发已认证的 GET/HEAD 路径，包括页面的数据读取；写请求、协议升级、service worker、WebSocket 与 HMR 暂不支持，`/va/*`、owner 页面、chat 与 review 不进入 Share。它不是通用 API 兼容层或 API 隔离沙盒；已接受的 GET/HEAD 路径不会按名称分类。
4. Provider children 和其它 child 一样注册清理；daemon 死亡不能留下 `cloudflared` 或 `tailscale funnel` 进程。
5. Public URL 是数据，不是 identity：消费者订阅变化，不要跨 restart 缓存它。

## 已知技术债

- Localtunnel 必须接收 daemon 实际 bound port；修复前自定义端口不得启 tunnel。
- Provider 仍需补显式 `Starting` 状态、URL 失效和有界 backoff；`Running`、`AwaitingApproval`、`Failed`、`Stopped` 已有明确表示。
- Orchestration 应迁入 injected server `TunnelService`，core 不应决定暴露哪个 listener。

---

*Source anchors: `src/core/src/tunnels/` (mod, providers/), `src/core/src/config.rs` (tunnel settings), `src/server/src/lib.rs` (boot wiring).*
*Last verified: 2026-07-22.*

<sub>[◀ Module: previews](previews.md) · [文档索引](../../README.md) · [Module: auth ▶](auth.md)</sub>
