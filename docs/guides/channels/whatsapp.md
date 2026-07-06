# Connect WhatsApp

WhatsApp connects through Baileys (an unofficial WhatsApp Web client) with QR login on first run; the session persists to disk afterwards.

## Setup

1. Add the config block below and start VibeAround.
2. Scan the QR code shown on first run with the WhatsApp mobile app (Linked devices).

## Configuration

Required fields: none (QR authentication at runtime).

```json
{
  "channels": {
    "whatsapp": {
      "verbose": { "show_thinking": false, "show_tool_use": false }
    }
  }
}
```

## Behavior

- Unofficial-client caveat: WhatsApp does not offer an official bot API for this use; treat the connection as best-effort and keep the linked device list tidy.

---

*Source anchors: [va-plugin-channel-whatsapp](https://github.com/jazzenchen/va-plugin-channel-whatsapp) `src/main.ts` (QR auth, session persisted).*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
