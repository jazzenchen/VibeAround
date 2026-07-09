# Download VibeAround

The current stable desktop release is [VibeAround v0.7.13](https://github.com/jazzenchen/VibeAround/releases/tag/v0.7.13), published on 2026-07-09.

GitHub Releases are the canonical source for release notes, source archives, checksums, and historical builds. The website also exposes a machine-readable release manifest at `https://vibearound.ai/releases/latest.json`.

## Recommended downloads

| Platform | Package | Direct link |
|---|---|---|
| macOS Apple Silicon | DMG | [VibeAround-macOS-arm64-0.7.13.dmg](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.13/VibeAround-macOS-arm64-0.7.13.dmg) |
| Windows x64 | Setup EXE | [VibeAround-Windows-x64-Setup-0.7.13.exe](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.13/VibeAround-Windows-x64-Setup-0.7.13.exe) |
| Windows x64 | MSI | [VibeAround-Windows-x64-MSI-0.7.13.msi](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.13/VibeAround-Windows-x64-MSI-0.7.13.msi) |
| Windows x64 | Portable ZIP | [VibeAround-Windows-x64-Portable-0.7.13.zip](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.13/VibeAround-Windows-x64-Portable-0.7.13.zip) |
| Linux x64 | AppImage | [VibeAround-Linux-x64-AppImage-0.7.13.AppImage](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.13/VibeAround-Linux-x64-AppImage-0.7.13.AppImage) |
| Linux x64 | deb | [VibeAround-Linux-x64-DEB-0.7.13.deb](https://github.com/jazzenchen/VibeAround/releases/download/v0.7.13/VibeAround-Linux-x64-DEB-0.7.13.deb) |

## Release highlights

Version 0.7.13 adds profile-scoped API configuration, updates the ACP bridge/runtime stack, and improves web chat, mobile pairing, local bridge, and route recovery reliability.

The command-line distribution is published separately as [VibeAround CLI 0.0.3](https://github.com/jazzenchen/VibeAround/releases/tag/va-v0.0.3). Use the desktop links above for the packaged app, and the CLI release assets for terminal-first installs. If you prefer npm, install the current CLI with:

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

*Source anchors: GitHub release `v0.7.13` (desktop packages), GitHub release `va-v0.0.3` (CLI packages), `src/npm/cli/` (npm CLI packaging), website `lib/release.ts` and `public/_redirects` (short download routes).*
*Last verified: v0.7.13*

<sub>[◀ Install and onboarding](install-and-onboarding.md) · [Documentation index](../README.md) · [Quick tour ▶](quick-tour.md)</sub>
