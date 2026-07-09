# Module: auth

`src/core/src/auth/`：守住每个界面的两种凭据：per-boot daemon token 和短寿命 pairing codes。策略讨论见[安全模型](../../architecture/security-model.md)。

## 职责

生成并持久化 daemon auth token，管理让远程浏览器获得该 token 的 pairing-code table。Enforcement（middleware）在 [server](server.md)；本模块是 source of truth。

## 关键类型

| Type | File | Role |
|---|---|---|
| `AuthToken` | `token.rs` | 每次 daemon start 都随机生成的 bearer token |
| `write_token_file` | `mod.rs` | 把 `{port, token}` 持久化到 `~/.vibearound/auth.json`，供进程外消费者使用（tray、CLI、desktop-ui） |
| `pair` | `pair.rs` | 6 位 codes，60 秒 TTL，通过 trusted surface 验证；`validate(code)` 成功时返回 token |

## 交互

- **← server：** `require_auth` middleware 检查 token（header 或 `?token=`）；pairing HTTP flow 驱动 code lifecycle。
- **← channels：** 聊天中的 `/pair <code>` 是 trusted confirmation path。
- **← cli：** `va pair` flows；`va auth` 读取/清理保存的文件。
- **← desktop：** 读取 token file，预认证打开 dashboard。

## 不变量：不要破坏

1. **Token 每次 daemon start 都轮换**，并立即覆盖文件；stale URLs 必须失败。不要跨 restart 持久化 token。
2. **Pairing codes 近似一次性且 60 秒**：purge-on-access 让表保持干净；code 不能活过自己的窗口。
3. **Confirmation 必须来自已经 trusted 的 surface**（local origin 或 connected chat）。新增 confirmation path 等于新增 trust assumption，要想清楚。
4. Token file 设计上是 plaintext（home-directory trust level）；不要往里放其它 secret。

## 已知技术债

- remediation plan 中无跟踪项。

---

*Source anchors: `src/core/src/auth/` (token, pair, mod), `src/server/src/web_server/auth.rs` (enforcement), `src/server/src/web_server/pair.rs` (HTTP flow).*
*Last verified: v0.7.11*

<sub>[◀ Module: tunnels](tunnels.md) · [文档索引](../../README.md) · [Module: server ▶](server.md)</sub>
