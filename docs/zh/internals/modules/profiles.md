# Module: profiles

`src/core/src/profiles/`：模型供应商配置，包括 profile 是什么、内置 provider catalog，以及 profile 如何在启动时变成具体 env/config/URLs。Bridge 的 serving side 在 [server](server.md)；本模块负责数据和渲染。

## 职责

定义 profile schema，随包提供 provider catalog，持久化用户 profiles，并把（profile × agent × launch target）渲染成一次 launch 需要的材料：环境变量、每个 agent 的 config overlays、alias model routes 和 local-api base URLs。

## 关键类型

| Type | File | Role |
|---|---|---|
| `ProfileDef` | `schema.rs` | 已保存 profile：provider、endpoint、auth mode、models、routes |
| `catalog` | `catalog.rs` | Embedded provider catalog（12 providers × endpoints × models × capabilities），从 `src/resources/profile-catalog/` 加载 |
| `connections` | `connections/` | 用户 profile store + model route resolution（`ProfileBridgeModelRoute`） |
| `render` / `RenderedProfile` | `render.rs` | 通用渲染：env targets、settings files、args |
| `bridge_launch` | `bridge_launch.rs` | 按 launch-target 渲染：Claude/Codex/Gemini/opencode/pi（含 desktop variants）config shapes、local-api URL minting |
| `runtime` | `runtime.rs` | 服务 bridge requests 时使用的 runtime lookups |
| `google_oauth` | `google_oauth.rs` | Gemini OAuth/code-assist credential flow |
| `headers` | `headers.rs` | 每个 provider 的 merged upstream headers |

## 交互

- **← agent：** `agent/launch.rs` 为 hosted spawns 和 native launches 调 materialization。
- **← server (api_bridge)：** upstream endpoint resolution、model mapping inputs、content policy 所需 capabilities。
- **← desktop / HTTP API：** profile CRUD。
- **→ config：** URL rendering 所需 port 和 bridge settings。

## 不变量：不要破坏

1. **Credentials 不离开 daemon**：渲染给 client 的 configs 只带 local URLs 和 alias ids；真实 keys 只在 upstream-side 附加。任何新 render target 都必须保留这一点。
2. **Catalog 是数据**：新增 providers/models 应是 catalog JSON，而不是代码，除非确实需要新 auth flow（如 google_oauth）或 dialect quirks（provider adapters）。
3. **Alias model ids 对每个 profile 要稳定**：launched CLIs 会把它们持久化在自己的 configs 里；改名会破坏已有 launches。
4. **`direct` 表示没有 profile**，不是空字段 profile；判断走 `profile_uses_vibearound_credentials`。

## 已知技术债

- `127.0.0.1:{port}/va/local-api/…` URL shape 和 server route table 重复维护，需要 shared contract + cross-layer test。
- Upstream auth 是每个 profile 静态 API key；Gemini adapter 之外的 subscription/OAuth upstreams 需要新 adapters（已知限制，也记录在 api-bridge memory 里）。
- `bridge_launch.rs` 有 1.3k 行 per-target rendering；下一个 target 落地时可考虑拆 per-target submodules。

---

*Source anchors: `src/core/src/profiles/` (schema, catalog, connections/, render, bridge_launch, runtime, google_oauth, headers), `src/resources/profile-catalog/`.*
*Last verified: v0.7.11*

<sub>[◀ Module: agent](agent.md) · [文档索引](../../README.md) · [Module: pty ▶](pty.md)</sub>
