# 桌面应用指南

桌面应用（Tauri）是包裹守护进程的管理外壳：它内嵌与 CLI 相同的服务端，为所有可配置项提供 GUI 页面，并从托盘维持运行时常驻。如果说守护进程是引擎，桌面应用就是仪表盘 *加* 点火钥匙。

## 服务管理

- 应用启动时拉起内嵌守护进程并显示其健康状态。从托盘/应用重启服务会执行干净关停（Agent、插件、预览全部收尾），再以新的认证 token 重新启动。
- **托盘/菜单栏：** 打开控制台（已预认证）、重启服务、退出。关闭主窗口不影响守护进程运行。
- Windows 上重启时的端口冲突会自动重试（该系统释放监听端口较慢）。

## 引导与 Startkit

首次启动会依次引导工具链、Agent、Profile 和渠道 —— 见[安装与上手](install-and-onboarding.md)。之后可能还会回头调整的两项：

- **工具链。** `system` 模式使用你自己的 Node.js；`managed` 模式由 VibeAround 安装并维护一份私有工具链（适合不能或不想全局管理 Node 的机器）。可在设置中切换。
- **Startkit。** 安装先决条件和 Agent CLI 的平台脚本；安装状态按条目跟踪汇报，机器只装了一半时能清楚看到缺什么。

## 模型 Profile

Profile 页面管理完整的 Profile 生命周期 —— 从供应商目录创建或自定义端点、编辑凭据和模型、排序、删除。改动对新的启动和宿主切换立即生效，无需重启守护进程。细节与配对建议：[模型 Profile 指南](model-profiles.md)。

## Agent 启动

Launch 页面把 Agent + Workspace + Profile 渲染成一次原生终端启动，支持终端偏好（Terminal.app/iTerm2/PowerShell/Linux 各终端）和按 Agent 的默认参数。它还列出**可恢复的会话** —— 包括在 VibeAround 之外创建的 —— 并提供归档/取消归档控制。细节：[Agent 启动指南](agent-launch.md)。

桌面版 Agent（`claude-desktop`、`codex-desktop`）作为已安装的厂商应用单独检测，以 GUI 应用方式启动，支持的场合可叠加 Profile 配置。

## 渠道与插件

插件管理器负责安装、更新和移除渠道插件；渠道页面编辑 `channels.<kind>` 配置，控制插件生命周期（启动/停止/重启/同步），并显示来自监督器的实时状态 —— 相当于 `va channels` / `va channel *` 的 GUI 版本。平台侧的准备步骤：[连接渠道](connect-channels.md)。

## 设置

设置页面编辑 `~/.vibearound/settings.json` 的各字段 —— Workspace、默认 Agent、隧道供应商与凭据、代理、搜索工具、集成开关（[配置参考](../reference/configuration.md)记录了每个字段）。应用运行时手动编辑文件也没问题；用重载操作（或 `va settings reload`）使其生效。

## 桌面应用在哪些场合是可选的

应用管理的一切也都能无界面地完成：`va serve` + `settings.json` + Web 控制台足以覆盖服务器和远程机器。桌面应用的独特价值在于内嵌的生命周期管理（不用守着终端）、原生启动集成和引导式上手。

---

*Source anchors: `src/desktop/src/` (main, tray, onboarding/, profiles/, startkit/), `src/desktop-ui/src/` (screens), `src/core/src/toolchain.rs` (system/managed), `src/desktop/src/desktop_detection.rs` (vendor apps).*
*Last verified: v0.7.11*

<sub>[◀ 快速导览](quick-tour.md) · [文档索引](../README.md) · [Web 控制台指南 ▶](web-dashboard.md)</sub>
