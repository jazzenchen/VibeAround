# 安装与上手

运行 VibeAround 有两种方式：**桌面应用**（推荐 —— GUI 管理 + 内嵌服务端）或 **npm CLI**（无界面服务端，适合终端优先的环境和远程机器）。两者产出同一个守护进程、同一个数据目录，之后随时可以切换，不丢任何东西。

## 方式一：桌面应用

从 [GitHub releases 页面](https://github.com/jazzenchen/VibeAround/releases)下载对应平台的安装包：

| 平台 | 安装包 |
|---|---|
| macOS Apple Silicon | DMG |
| Windows x64 | Setup EXE、MSI 或便携版 ZIP |
| Linux x64 | AppImage 或 deb |

macOS Intel 目前只能源码构建 —— 见[源码构建](build-from-source.md)。

安装并启动。应用常驻托盘/菜单栏；关闭窗口不会停止守护进程。

### 首次运行引导

桌面应用会在首次启动时引导你完成设置：

1. **工具链检查。** VibeAround 需要 Node.js 来运行 Agent 的 ACP 适配器和渠道插件。引导会检测系统工具链；如果你不想动系统环境，也可以安装一份托管工具链（`toolchain` 设置：`system` 或 `managed`）。
2. **Agent 检测。** 已安装的 Agent CLI（Claude Code、Codex、Gemini CLI 等）会在 PATH 上被检测出来；启用的 Agent 在启动时获得项目级 VibeAround skills 和 MCP 配置。
3. **模型 Profile（可选）。** 现在添加一个供应商凭据，或先跳过，让 Agent 用自己的官方登录（即 `direct` Profile）。
4. **渠道（可选）。** 安装并配置 IM 渠道插件。每个渠道都需要平台侧的准备（bot token 等），见[连接渠道](connect-channels.md)；这一步随时可以补做。

引导结束后，控制台会在浏览器中打开，已完成认证。接下来可以开始[快速导览](quick-tour.md)。

## 方式二：npm CLI

```bash
npm i -g @vibearound/cli
```

这会安装 `va` 和 `vibearound` 命令、原生服务端二进制、`va-launch` 原生启动器、TUI，以及预构建的 Web 控制台。

启动服务：

```bash
va serve
```

守护进程绑定 `127.0.0.1:12358`，把认证 token 写入 `~/.vibearound/auth.json`，并在 `http://127.0.0.1:12358/va/` 提供控制台（带 token 打开 —— `va status` 会显示 URL）。配置文件是 `~/.vibearound/settings.json`，首次运行时以默认值创建；这种模式没有 GUI 引导：通过 Web 控制台配置渠道和 Profile，或直接编辑 settings.json（见[配置参考](../reference/configuration.md)）。专门的 CLI 配置命令在计划中。用桌面应用则完全不需要编辑文件 —— 配置都在 UI 里完成。

有用的初始命令：

```bash
va status      # 运行时摘要：渠道、隧道、Agent、会话
va doctor      # 诊断端点、认证和服务健康状态
va agents      # 列出已启用的 Agent
vibearound tui # 终端 UI
```

## 数据目录

VibeAround 持久化的一切都在 `~/.vibearound/`（可用 `VIBEAROUND_DATA_DIR` 覆盖）：

```text
~/.vibearound/
├── settings.json         # 主配置
├── auth.json             # 控制台、MCP、Bridge、Agent-as-API 四把 token
├── agents.json           # 检测到的 Agent 可执行文件
├── plugins/              # 已安装的渠道插件
├── workspaces/           # 新建 Workspace 的默认根目录
├── launch/profiles/      # 保存的 va-launch 启动配置
└── *.jsonl               # workspace/thread/attachment 事件日志
```

卸载应用或 npm 包不会删除这个目录；想彻底清空需手动删除。

## 升级

- **桌面应用：** 直接用新包覆盖安装；数据目录不受影响。
- **npm：** `npm i -g @vibearound/cli@latest`。
- 大版本升级前先看 release notes 是否有破坏性变更 —— 设置迁移会在守护进程启动时自动运行，但渠道插件可能需要事后执行 `va channel sync`。

---

*Source anchors: `src/npm/cli/` (package contents), `src/core/src/config.rs` (data_dir, DEFAULT_PORT, settings bootstrap), `src/desktop/src/onboarding/` (onboarding steps), `src/core/src/toolchain.rs` (system/managed), `src/cli/src/args.rs` (va commands).*
*Last verified: v0.7.11*

<sub>[◀ 支持矩阵](../product/supported-matrix.md) · [文档索引](../README.md) · [快速导览 ▶](quick-tour.md)</sub>
