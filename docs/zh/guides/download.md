# 下载 VibeAround

当前稳定桌面版本是 [VibeAround v0.7.24](https://github.com/jazzenchen/VibeAround/releases/tag/v0.7.24)，发布时间为 2026-08-07。

GitHub Releases 是 release notes、源码归档、历史版本和校验信息的主来源。网站也提供机器可读的 release manifest：`https://vibearound.ai/releases/latest.json`。

## 推荐下载

| 平台 | 安装包 | 直达链接 |
|---|---|---|
| macOS Apple Silicon | DMG | [VibeAround-macOS-arm64-0.7.24.dmg](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.24/VibeAround-macOS-arm64-0.7.24.dmg) |
| Windows x64 | Setup EXE | [VibeAround-Windows-x64-Setup-0.7.24.exe](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.24/VibeAround-Windows-x64-Setup-0.7.24.exe) |
| Windows x64 | MSI | [VibeAround-Windows-x64-MSI-0.7.24.msi](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.24/VibeAround-Windows-x64-MSI-0.7.24.msi) |
| Windows x64 | Portable ZIP | [VibeAround-Windows-x64-Portable-0.7.24.zip](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.24/VibeAround-Windows-x64-Portable-0.7.24.zip) |
| Linux x64 | AppImage | [VibeAround-Linux-x64-AppImage-0.7.24.AppImage](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.24/VibeAround-Linux-x64-AppImage-0.7.24.AppImage) |
| Linux x64 | deb | [VibeAround-Linux-x64-DEB-0.7.24.deb](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.24/VibeAround-Linux-x64-DEB-0.7.24.deb) |

## 版本要点

v0.7.24 修复了 0.7.23 引入的多模态图片解析器的协议处理。现在会完整保留所选的 Profile、API 类型和模型，并针对 OpenAI Chat Completions、OpenAI Responses、Anthropic Messages 和 Gemini GenerateContent Profile 使用对应协议的 endpoint、认证、请求编码和响应解析。OpenAI Responses Profile 的连接测试也改用了有效的输出 token 上限。现有配置无需迁移。

命令行发行包单独发布为 [VibeAround CLI 0.0.13](https://github.com/jazzenchen/VibeAround/releases/tag/va-v0.0.13)。需要桌面应用时用上面的安装包；terminal-first 场景可安装对应的 npm 包：

```bash
npm i -g @vibearound/cli
```

## 下载路由

网站提供几个短路由：

| 路由 | 指向 |
|---|---|
| `https://vibearound.ai/download` | 当前桌面版 GitHub Release 页面 |
| `https://vibearound.ai/download/mac` | macOS Apple Silicon DMG |
| `https://vibearound.ai/download/windows` | Windows x64 Setup EXE |
| `https://vibearound.ai/download/linux` | Linux x64 AppImage |

这些路由只是便捷入口。需要明确文件时，以本页的直达链接和 GitHub Release 页面为准。

---

*Source anchors: GitHub release `v0.7.24` (desktop packages), GitHub release `va-v0.0.13` (CLI packages), `src/npm/cli/` (npm CLI packaging), website `lib/release.ts` and `public/_redirects` (short download routes).*
*Last verified: v0.7.24*

<sub>[◀ 安装与上手](install-and-onboarding.md) · [文档索引](../README.md) · [快速导览 ▶](quick-tour.md)</sub>
