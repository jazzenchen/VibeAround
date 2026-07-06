# Module: tunnels

`src/core/src/tunnels/`：通过三个可互换 provider，把本地 dashboard 发布到 public URL。

## 职责

启动、跟踪、停止 tunnel runtimes；向系统其它部分暴露当前 public URL（pairing hints、preview link bases）。每个 provider 的 process model 不同，通过统一的 start/guard interface 隐藏。

## 关键类型

| Type | File | Role |
|---|---|---|
| `TunnelManager` | `mod.rs` | Live tunnels registry：provider → URL、registry id、abort handle；实现 `StateSource` |
| `start_web_tunnel_with_provider` | `mod.rs` | 入口：config → provider start → (guard, public URL) |
| ngrok provider | `providers/ngrok.rs` | 通过 ngrok Rust SDK 进程内运行（session + forwarder；可选 reserved domain） |
| cloudflare provider | `providers/cloudflare.rs` | 子进程：`cloudflared tunnel run --token …` |
| localtunnel provider | `providers/localtunnel.rs` | 子进程：`npx localtunnel --port 12358` |

## 交互

- **← server (daemon boot)：** 启动配置的 tunnel，注册 abort handle；`stop()` abort 并清空。
- **← auth：** 存在 public hostname 时触发 pairing gate。
- **← previews / dashboard：** share links 和展示使用 public URL（`preview_base_url` 可覆盖）。
- **→ resources：** provider program definitions 和 spawn-error hints（例如“is Node/npx installed?”）。

## 不变量：不要破坏

1. **`none` 是一等 provider**：不跑 tunnel code，不 spawn child；新 call sites 必须容忍 URL 缺失。
2. **Tunnel 只暴露 web listener**：不要通过它绑定额外端口；loopback-only surfaces（local-api）必须保持不可达。
3. Provider children 和其它 child 一样注册清理；daemon 死亡不能留下 `cloudflared`。
4. Public URL 是数据，不是 identity：消费者订阅变化，不要跨 restart 缓存它。

## 已知技术债

- remediation plan 中无跟踪项。（三个 provider 的集合是有意设计；新增 provider 走同一 guard pattern。）

---

*Source anchors: `src/core/src/tunnels/` (mod, providers/), `src/core/src/config.rs` (tunnel settings), `src/server/src/lib.rs` (boot wiring).*
*Last verified: v0.7.11*

<sub>[◀ Module: previews](previews.md) · [文档索引](../../README.md) · [Module: auth ▶](auth.md)</sub>
