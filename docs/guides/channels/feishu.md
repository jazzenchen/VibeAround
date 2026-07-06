# Connect Feishu / Lark

Feishu and Lark share one open-platform model, but the portal and API domain must match your tenant: mainland Feishu uses the [Feishu Open Platform](https://open.feishu.cn/) (`open.feishu.cn`); overseas Lark uses [Lark Developer](https://open.larksuite.com/) (`open.larksuite.com`). Keep the app, its permissions, event/callback subscriptions, and the credentials you paste into VibeAround **all on the same platform** — a Feishu app against the Lark domain (or the reverse) fails to connect.

VibeAround uses **long connection** (WebSocket initiated from your machine) for both events and callbacks, so no public callback URL or tunnel is required.

## Platform setup

1. Open the matching developer portal and create an **enterprise internal app**.
2. Enable the **bot capability**, then copy the App ID and App Secret.
3. In *Development Configuration → Permissions & Scopes*, use **batch import** with the permission JSON below.
4. In *Development Configuration → Events and Callbacks → Event Configuration*, choose **long connection** and add `im.message.receive_v1` (delivers user messages to VibeAround).
5. In *Callback Configuration*, choose **long connection** and add `card.action.trigger` (delivers card button taps — approve/deny, session selection).
6. In *Version Management*, **create and publish a version** — permissions, events, and callbacks usually take effect only after publishing, and enterprise tenants may require admin approval.
7. Add the bot to a controlled private chat or small group first; verify message delivery and card buttons before broad rollout.

Permission batch-import JSON:

```json
{
  "scopes": {
    "tenant": [
      "aily:file:read",
      "aily:file:write",
      "application:application.app_message_stats.overview:readonly",
      "application:application:self_manage",
      "application:bot.menu:write",
      "cardkit:card:write",
      "contact:user.employee_id:readonly",
      "corehr:file:download",
      "event:ip_list",
      "im:chat.access_event.bot_p2p_chat:read",
      "im:chat.members:bot_access",
      "im:message",
      "im:message.group_at_msg:readonly",
      "im:message.p2p_msg:readonly",
      "im:message:readonly",
      "im:message:send_as_bot",
      "im:resource"
    ],
    "user": [
      "aily:file:read",
      "aily:file:write",
      "im:chat.access_event.bot_p2p_chat:read"
    ]
  }
}
```

## Configuration

Required fields: `app_id`, `app_secret`.

```json
{
  "channels": {
    "feishu": {
      "app_id": "cli_xxxxxxxxxxxx",
      "app_secret": "your-app-secret",
      "verbose": { "show_thinking": true, "show_tool_use": true }
    }
  }
}
```

## Behavior

- Replies render as **V2 interactive cards** that update in place while the agent streams; permission requests arrive as tappable card buttons.
- Group chats and DMs are separate conversation routes; multiple bots in one group each keep their own thread.

## Troubleshooting

| Symptom | Check |
|---|---|
| No incoming messages | Event Configuration uses long connection and includes `im.message.receive_v1` |
| Card buttons do nothing | Callback Configuration uses long connection and includes `card.action.trigger` |
| Imported permissions still fail | Publish a new app version; confirm admin approval went through |
| Long-connection validation fails | Start VibeAround first (the plugin initiates the connection); confirm the machine reaches the matching open-platform domain |
| Credentials fail to connect | App ID/Secret, developer portal, and API domain must all belong to the same platform (Feishu vs Lark) |

Official references: [API permissions](https://open.feishu.cn/document/server-docs/application-scope/introduction) · [Events over long connection](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case) · [Callbacks over long connection](https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address) · [Receive message event](https://open.larksuite.com/document/uAjLw4CM/ukTMukTMukTM/reference/im-v1/message/events/receive) · [Card interactions](https://open.feishu.cn/document/feishu-cards/configuring-card-interactions)

---

*Source anchors: [va-plugin-channel-feishu](https://github.com/jazzenchen/va-plugin-channel-feishu) `src/main.ts` (requiredConfig: app_id, app_secret); permission JSON and subscription names from the verified website draft (2026-06); V2 card requirement per the Feishu platform.*
*Last verified: v0.7.11*

<sub>[◀ Connect channels](../connect-channels.md) · [Documentation index](../../README.md)</sub>
