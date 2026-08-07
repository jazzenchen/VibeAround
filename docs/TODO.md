# Docs TODO

## Screenshots (owner: @jazzenchen — capture, then drop into `docs/assets/` and reference)

Priority order; suggested capture at ~1440px width, light theme, redact any real tokens/chat names.

| # | Screenshot | Goes into | What it must show |
|---|---|---|---|
| 1 | Desktop onboarding — toolchain step | guides/install-and-onboarding.md §First-run onboarding | The system/managed choice |
| 2 | Desktop onboarding — agent detection | same | Detected agents with enable toggles |
| 3 | Model profile creation (provider picker + endpoint variant) | guides/model-profiles.md §Creating a profile | Catalog list + one provider's endpoint groups |
| 4 | Launch screen | guides/agent-launch.md §Launching from the desktop app | Agent + workspace + profile + terminal selection |
| 5 | Web Chat with a permission card | guides/web-dashboard.md §Web Chat | Streaming turn + inline permission card |
| 6 | Web Terminal with tabs | guides/web-dashboard.md §Web Terminal | Two tabs, one attached session |
| 7 | IM permission card (Telegram or Feishu) | guides/im-usage.md §The basics | Tappable card in a real chat |
| 8 | Pairing gate on a tunneled URL | guides/tunnels-and-remote-access.md §First visit | 6-digit code screen |
| 9 | Channel plugin manager | guides/connect-channels.md §The pattern | Installed plugins with status |
| 10 | Preview list with Server and Markdown rows | guides/web-dashboard.md §Live Preview | Server local-only state + Markdown owner/share controls |

When adding: `![alt text](../assets/<name>.png)` under the matching section; keep one image per section maximum.

## Deferred

- When CLI configuration commands ship (channel/profile setup without editing settings.json), update: quick-tour §2, install-and-onboarding (npm section), connect-channels, reference/cli.

- Chinese translations (`-CN` pages) — deliberately postponed; prioritize `connect-channels`, `im-usage`, and the Feishu/WeCom/DingTalk-relevant guides when the batch starts.
