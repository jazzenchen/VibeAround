# Download VibeAround

The current stable desktop release is [VibeAround v0.7.21](https://github.com/jazzenchen/VibeAround/releases/tag/v0.7.21), published on 2026-08-01.

GitHub Releases are the canonical source for release notes, source archives, checksums, and historical builds. The website also exposes a machine-readable release manifest at `https://vibearound.ai/releases/latest.json`.

## Recommended downloads

| Platform | Package | Direct link |
|---|---|---|
| macOS Apple Silicon | DMG | [VibeAround-macOS-arm64-0.7.21.dmg](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.21/VibeAround-macOS-arm64-0.7.21.dmg) |
| Windows x64 | Setup EXE | [VibeAround-Windows-x64-Setup-0.7.21.exe](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.21/VibeAround-Windows-x64-Setup-0.7.21.exe) |
| Windows x64 | MSI | [VibeAround-Windows-x64-MSI-0.7.21.msi](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.21/VibeAround-Windows-x64-MSI-0.7.21.msi) |
| Windows x64 | Portable ZIP | [VibeAround-Windows-x64-Portable-0.7.21.zip](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.21/VibeAround-Windows-x64-Portable-0.7.21.zip) |
| Linux x64 | AppImage | [VibeAround-Linux-x64-AppImage-0.7.21.AppImage](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.21/VibeAround-Linux-x64-AppImage-0.7.21.AppImage) |
| Linux x64 | deb | [VibeAround-Linux-x64-DEB-0.7.21.deb](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.21/VibeAround-Linux-x64-DEB-0.7.21.deb) |

## Release highlights

Version 0.7.21 lets you manually restore ChatGPT or Claude desktop agents when automatic detection misses them. On Windows, a searchable graphical picker selects Start menu apps without requiring an App ID. The release also adds skippable onboarding install items and refreshes stale desktop-app detection results.

The command-line distribution is published separately as [VibeAround CLI 0.0.10](https://github.com/jazzenchen/VibeAround/releases/tag/va-v0.0.10). Use the desktop links above for the packaged app, and the CLI release assets for terminal-first installs. npm currently serves 0.0.9; install it with:

```bash
npm i -g @vibearound/cli
```

## Download routes

The website exposes short routes for common packages:

| Route | Target |
|---|---|
| `https://vibearound.ai/download` | Current desktop GitHub release page |
| `https://vibearound.ai/download/mac` | macOS Apple Silicon DMG |
| `https://vibearound.ai/download/windows` | Windows x64 setup EXE |
| `https://vibearound.ai/download/linux` | Linux x64 AppImage |

These routes are convenience entry points. When in doubt, use the direct package links above or the GitHub release page.

---

*Source anchors: GitHub release `v0.7.21` (desktop packages), GitHub release `va-v0.0.10` (CLI packages), `src/npm/cli/` (npm CLI packaging), website `lib/release.ts` and `public/_redirects` (short download routes).*
*Last verified: v0.7.21*

<sub>[◀ Install and onboarding](install-and-onboarding.md) · [Documentation index](../README.md) · [Quick tour ▶](quick-tour.md)</sub>
