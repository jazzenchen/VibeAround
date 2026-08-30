# Module: auth

`src/core/src/auth/`（当前位置）：server surfaces 使用的 owner token、本地 API scoped token 与短寿命 pairing codes。策略讨论见[安全模型](../../architecture/security-model.md)。

## 职责

生成并持久化 daemon auth token，管理 pairing-code table。它在语义上是 **server authentication capability**；middleware、pairing HTTP flow 和 token lifetime 最终应由同一个 injected `AuthService` 拥有。

## 关键类型

| Type | File | Role |
|---|---|---|
| `AuthToken` | `token.rs` | 每次 daemon start 都随机生成的 bearer token |
| `SharedAuthToken` | `token.rs` | 各 handler 共享、可在运行时被用户轮换的 token |
| `write_auth_file` | `mod.rs` | 一次写入把四把凭据持久化到 `~/.vibearound/auth.json`，供进程外消费者使用（tray、CLI、desktop-ui） |
| `read_mcp_token_file` / `read_local_api_token_file` / `read_local_agent_api_token_file` | `mod.rs` | 从该文件中取出对应的单把凭据 |
| `pair` | `pair.rs` | 6 位 codes，60 秒 TTL，通过 trusted surface 验证；`validate(code)` 成功时返回 token |

## 交互

- **← server：** `require_auth` middleware 检查 token（header 或 `?token=`）；pairing HTTP flow 驱动 code lifecycle。
- **← channels：** 聊天中的 `/pair <code>` 是 trusted confirmation path。
- **← cli：** `va pair` flows；`va auth` 读取/清理保存的文件。
- **← desktop：** 读取 token file，预认证打开 dashboard。

## 不变量：不要破坏

1. **当前 token 生命周期是 `ServerDaemon` 生命周期**，不是每次 `start_background` generation；Desktop hot restart 会复用同一个 daemon 对象的 token，新对象/进程才轮换。
2. **Pairing codes 近似一次性且 60 秒**：purge-on-access 让表保持干净；code 不能活过自己的窗口。
3. **Confirmation 必须来自已经 trusted 的 surface**（local origin 或 connected chat）。新增 confirmation path 等于新增 trust assumption，要想清楚。
4. Token file 设计上是 plaintext（home-directory trust level）；不要往里放其它 secret。

## 已知技术债

- 将策略收敛为 server-owned `AuthService`，core 只保留最小 primitive。
- Pairing/global memory tables 需要容量限制和 active-code 唯一性。
- Token/settings 文件需要 secure-create + atomic replace。

---

*Source anchors: `src/core/src/auth/` (token, pair, mod), `src/server/src/web_server/auth.rs` (enforcement), `src/server/src/web_server/pair.rs` (HTTP flow).*
*Last verified: system review 2026-07-11.*

<sub>[◀ Module: tunnels](tunnels.md) · [文档索引](../../README.md) · [Module: server ▶](server.md)</sub>
