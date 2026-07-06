# 源码构建

从代码检出构建 VibeAround 的任何部分 —— 独立服务端、CLI 工具或完整桌面应用。这是 macOS Intel 的受支持路径，也是开发路径。

## 先决条件

- **Rust**（stable 工具链）—— 工作区有七个 crate
- **Bun** —— 驱动 JS 构建和工作区脚本
- **Node.js** —— 渠道插件和 Agent ACP 适配器的运行时
- **Tauri 系统依赖** —— 仅桌面应用需要（按平台见 [Tauri prerequisites](https://tauri.app/start/prerequisites/)：macOS 上 Xcode CLT、Windows 上 MSVC、Linux 上 webkit2gtk）

下面所有构建命令都在仓库的 `src/` 下运行。

## 独立服务端 + 控制台

```bash
bun install
bun run web:build        # 控制台 SPA → web/dist
cargo build --release -p server
```

带上构建好的控制台运行：

```bash
cargo run --release -p server    # 绑定 127.0.0.1:12358
```

## CLI、TUI 与启动器

```bash
bun run va:build         # cargo build -p va-cli -p va-tui -p va-launcher
```

二进制落在 `target/debug/`（或 `--release`）：`va`、`va-tui`、`va-launch`。注意 `va launch` 会执行同目录的 `va-launch` 二进制 —— 保持它们在同一目录，打包发行版就是这么做的。

## 桌面应用

```bash
bun install
bun run build            # desktop-ui + web SPA，然后 tauri build
```

带 UI 热更新的开发模式：

```bash
bun run dev              # tauri dev（desktop-ui 由 vite 提供）
```

Tauri 构建会自动把 `va-launch` 准备为 sidecar 二进制（`scripts/prepare-va-launch.mjs`），并打出各平台的包（DMG/EXE/MSI/AppImage/deb）。

## 运行测试

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

JS 部分用 `bun run web:build` 和 `bun run desktop-ui:build` 做构建检查。

## tmux 集成（可选）

安装了 tmux 时，Web 终端就能附着到 tmux 会话 —— 不需要构建开关，运行时检测。`tmux_detach_others` 设置控制从控制台附着时是否把其他客户端踢下线。

## 没有维护者密钥就无法构建的部分

超出本地未签名构建的发布打包，用到维护者私有、刻意不入库的配置：

- **macOS 签名/公证**（`apple-sign` 配置）—— 本地 DMG 未签名也能用，但 Gatekeeper 会告警。
- **发布构建脚本**（`build.sh`）和更新通道发布。

完整可用的本地构建所需的一切都是公开的；只有发行签名是私有的。

## 插件与 SDK

渠道插件和 `@vibearound/plugin-channel-sdk` 是独立仓库，用它们自己的 npm 构建（那些仓库用 npm，不用 bun）。见[开发渠道插件](build-a-channel-plugin.md)。

---

*Source anchors: `src/package.json` (build scripts), `src/Cargo.toml` (workspace members), `src/scripts/prepare-va-launch.mjs` (sidecar), `src/npm/cli/` (npm packaging).*
*Last verified: v0.7.11*

<sub>[◀ 开发渠道插件](build-a-channel-plugin.md) · [文档索引](../README.md) · [故障排查与 FAQ ▶](troubleshooting-and-faq.md)</sub>
