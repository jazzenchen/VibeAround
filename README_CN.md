<div align="center">

# VibeAround

**一套本地优先的统一工作台，把桌面端、Web 和聊天式编码工作流连成一个运行时。**

[English](README.md) | [简体中文](README_CN.md) | [Wiki](https://github.com/jazzenchen/VibeAround/wiki)

<p>
  <img src="Logo.png" width="120" alt="VibeAround" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Bun-1.3+-000?style=flat-square&logo=bun&logoColor=fff" alt="Bun" />
  <img src="https://img.shields.io/badge/Rust-1.78+-000?style=flat-square&logo=rust&logoColor=fff" alt="Rust" />
  <img src="https://img.shields.io/badge/Vite-6-646CFF?style=flat-square&logo=vite&logoColor=fff" alt="Vite" />
  <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=000" alt="React" />
  <img src="https://img.shields.io/badge/License-MIT-blue?style=flat-square" alt="License: MIT" />
</p>

</div>

VibeAround 把 coding agent、terminal session 和远程访问入口整合进同一个运行时。它不是把桌面工具、浏览器工具和聊天入口简单并列，而是让它们共享同一套 session、配置模型和 agent 生命周期。

它适合这些场景：

- 在桌面端开始工作，再从浏览器继续
- 让长时间运行的 coding session 可以从多台设备访问
- 在 web chat、terminal 和 IM channels 之间切换，而不切换底层运行时模型
- 在一套产品里管理多个 coding agent backend

## 界面截图

| 桌面端 | 移动端 |
|--------|--------|
| <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/pc.webp" width="720" alt="VibeAround 桌面端 Web 控制台" /> | <img src="https://pub-806a1b8456464ce7a6c110f84946697e.r2.dev/screenshots/mobile-claude.webp" width="200" alt="VibeAround 移动端 Web 控制台" /> |

## 为什么是 VibeAround

很多编码工作流一旦跨设备、跨入口，就会迅速割裂：terminal 在一边，chat 在一边，agent 状态很难统一管理。

VibeAround 的做法，是把产品建立在共享运行时模型之上：

- 一套配置来源
- 一套 session 模型
- 一套以 channel 为核心的消息路由架构
- 多个面向用户的产品入口

## 当前可以做什么

- 打开 web dashboard 管理 terminal sessions 和 chat
- 启动或附加持久化 terminal sessions，包括基于 tmux 的工作流
- 在 web chat 界面中与支持的 coding agents 对话
- 通过 Telegram、飞书等 IM channels 接入同一套系统
- 在 desktop UI 中查看运行中的 agents、channels、tunnels 和 PTY sessions
- 在 onboarding 中选择启用哪些 agents，并设置默认 agent

## 产品入口

| 入口 | 作用 |
|---|---|
| Desktop app | 首次安装、onboarding、运行时可视化、tray actions |
| Web dashboard | 日常主工作区，用于 terminals、tmux sessions 和 web chat |
| IM channels | 通过 Telegram、飞书插件提供轻量级远程访问 |

## 当前支持的智能体

VibeAround 当前支持这些 coding agents：

- Claude Code
- Gemini CLI
- OpenCode
- Codex

是否启用某个 agent，以及默认 agent 是什么，均在 onboarding 中配置，并写入 `~/.vibearound/settings.json`。

## 快速开始

```bash
cd src
bun install
bun run prebuild
bun run dev
```

启动后：

1. 打开 desktop app
2. 首次运行时完成 onboarding
3. 选择启用的 agents 和默认 agent
4. 从托盘或 desktop UI 打开 web dashboard
5. 通过 terminals、tmux sessions 或 web chat 开始工作

## 配置位置

当前生效的运行时配置文件位于：

- `~/.vibearound/settings.json`

Channel plugins 的加载路径为：

- `~/.vibearound/plugins/<channel>/dist/main.js`

## 文档入口

README 只保留项目介绍与快速上手，完整技术手册请看 wiki。

建议阅读顺序：

- [Wiki 首页](https://github.com/jazzenchen/VibeAround/wiki)
- [安装与运行指南](https://github.com/jazzenchen/VibeAround/wiki/Setup-Guide-CN)
- [产品入口说明](https://github.com/jazzenchen/VibeAround/wiki/Product-Surfaces-CN)
- [架构说明](https://github.com/jazzenchen/VibeAround/wiki/Architecture-CN)
- [配置模型](https://github.com/jazzenchen/VibeAround/wiki/Configuration-Model-CN)
- [支持的智能体](https://github.com/jazzenchen/VibeAround/wiki/Supported-Agents-CN)
- [运行语义](https://github.com/jazzenchen/VibeAround/wiki/Operational-Semantics-CN)
- [构建与打包](https://github.com/jazzenchen/VibeAround/wiki/Build-and-Packaging-CN)

## 项目状态

VibeAround 仍在快速演进中。当前架构已经可用，但运行时行为和产品入口仍在持续打磨。

本仓库公开的主要目的是透明、学习和分享。当前阶段暂不接受 Pull Request 和 feature request。

## 开源协议

本项目基于 [MIT License](LICENSE) 开源。
