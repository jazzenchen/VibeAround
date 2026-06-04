# Prompt: Optimize Startkit Onboarding Copy

Use the prompt below with another AI to optimize the guiding copy for VibeAround Startkit onboarding.

```text
你是一名资深产品文案和本地化设计师。请帮我优化 VibeAround 的 Startkit onboarding 文案。

## 产品背景

VibeAround 是一个本机运行的 Coding Agent 工作台。它帮助用户在自己的电脑上启动 Claude Code、Codex CLI 等 Coding Agent，管理 API 配置，连接聊天工具入口，并在需要时配置远程访问，让用户可以从桌面端、手机或远程入口继续使用 Coding Agent。

Startkit 是 VibeAround 的首次启动安装和配置引导。它会自动检测用户电脑上已有的工具，复用系统里已经可用的内容，只安装缺少且用户选择的部分。安装完成后，再引导用户填写 API、聊天工具、远程访问等必要配置。

## 当前 UI 形式

这是一个 5 步 onboarding：

1. Agents：选择要准备的 Coding Agent。
2. IM：选择是否通过聊天工具访问 VibeAround。
3. Remote：选择是否开启远程访问，默认可只在本机使用。
4. Install：自动检测并安装缺少的选中项。
5. Config：填写前面选择过的 API、聊天工具、远程访问配置。

UI 是左右分屏：

- 左侧像 PPT section cover：一个大标题，一句简短说明，必要时有一句很轻的提示。
- 右侧是对应步骤的设置项：标题、简短说明、选项描述或空状态。
- 底部会常驻一句提示：不确定时请保持默认设置，所有设置后续均可更改。
- 我只需要优化引导性文案，不需要按钮文案、状态文案、安装进度文案、错误文案。

## 文案目标

请同时优化英文和中文文案。英文要自然、简洁、像成熟桌面应用；中文要像中文产品界面，不要翻译腔。

重点：

- 降低首次安装的压力，让用户知道默认选项是安全的。
- 不要过度解释技术细节，也不要营销腔。
- 尽量避免生硬词：IM、CLI、tunnel、registry、MCP server、handover skill。
- 但可以保留必要品牌名：VibeAround、Claude Code、Codex CLI、Cloudflare、ngrok。
- 中文里不要把 Coding Agent 翻译成“代码助手”，统一写作 Coding Agent。
- 不要承诺产品没有实现的能力。
- 每句都尽量短，适合放在 UI 里。
- 中文可以比英文更口语一点，但不要太随意。
- 保持 5 个步骤之间的节奏一致。

## 当前文案

### Shared Guidance

| Placement | Current English | Current Chinese |
|---|---|---|
| Footer hint | Keep the defaults if you are not sure; everything can be changed later. | 不确定时请保持默认设置，所有设置后续均可更改。 |

### Step 1: Agents

| Placement | Current English | Current Chinese |
|---|---|---|
| Left title | Start with your coding agents. | 选择 Coding Agent。 |
| Left description | Claude Code and Codex CLI are recommended for daily vibe coding and vibe coding jobs. | 推荐使用 Claude Code 和 Codex CLI 进行氛围编程与氛围办公。 |
| Right title | Coding Agent | Coding Agent |
| Right description | Choose the Coding Agents you want to use. | 选择你想使用的 Coding Agent。 |

说明：Agent 名称来自动态配置，例如 Claude Code、Codex CLI、Pi、Gemini CLI、Opencode、Cursor CLI、Kiro CLI、Qwen Code。

### Step 2: IM

| Placement | Current English | Current Chinese |
|---|---|---|
| Left title | Choose your IM entry points. | 选择消息入口。 |
| Left description | Pick the apps you use. Login and tokens wait until the final step. | 勾选你常用的聊天工具，登录信息稍后再填。 |
| Left hint | Skip this if you only plan to use the desktop app. | 只用桌面端的话，可以跳过这一步。 |
| Right title | IM access | 消息入口 |
| Right description | Select the messaging apps you actually use. | 选择你常用的聊天工具。 |
| Empty state | No channel plugins are available. | 暂时没有可安装的消息插件。 |

说明：插件名称和描述来自动态配置，例如 Telegram、Feishu (Lark)、Discord、Slack、WeChat、DingTalk、WeCom。

### Step 3: Remote

| Placement | Current English | Current Chinese |
|---|---|---|
| Left title | Decide on remote access. | 要不要开启远程访问？ |
| Left description | Cloudflare gives this machine a stable public route when you need one. | 需要从外面访问这台电脑时，推荐使用 Cloudflare。 |
| Left hint | Local-only setups can skip this step. | 只在本机使用就跳过。 |
| Right title | Remote access | 远程访问 |
| Right description | Choose how this computer can be reached from outside. | 选择是否允许从外部访问这台电脑。 |
| None option description | Keep everything local on this computer. | 不开放远程访问，只在本机使用。 |
| Cloudflare option description | Stable named tunnel with a public hostname. | 适合长期使用，会提供一个固定访问地址。 |
| ngrok option description | Useful when you already have an ngrok account and domain. | 已经在用 ngrok 时可以选择它。 |
| localtunnel option description | Quick temporary public URL for lightweight testing. | 临时测试用，地址可能会变化。 |
| Fallback option description | Remote access provider. | 远程访问方式。 |

说明：远程访问选项名称来自动态配置，目前常见顺序是 None、Cloudflare、ngrok、localtunnel。

### Step 4: Install

| Placement | Current English | Current Chinese |
|---|---|---|
| Left title | Let Startkit prepare the computer. | 开始安装需要的工具。 |
| Left description | The check runs automatically. Install only the selected pieces. | VibeAround 会先自动检测，只安装缺少的部分。 |
| Left hint | Details stay available, but the main flow stays simple. | 想看细节可以展开，不影响主流程。 |
| Install guidance | Some items only need configuration in the next step. | 有些内容只需要下一步填写信息。 |
| Install guidance | Ready items are skipped automatically. | 已经可用的工具会自动跳过。 |
| Empty/loading guidance | The environment check starts automatically. | 检测会自动开始。 |

说明：安装分组来自 Startkit plan，概念上包括基础工具、Coding Agent、消息工具和远程访问。

### Step 5: Config

| Placement | Current English | Current Chinese |
|---|---|---|
| Left title | Finish the parts that need you. | 填写最后几项信息。 |
| Left description | Add API profiles, IM login, or tunnel tokens only when selected. | 只会显示你前面选过的配置项。 |
| Left hint | Empty sections are hidden automatically. | 不需要的部分会自动隐藏。 |
| API section title | Agent API profiles | API 配置 |
| API section description | Optional. You can add or edit profiles from Launch later. | 可选。之后也可以在启动页里新增或编辑。 |
| API empty state | No API profiles yet. | 还没有 API 配置。 |
| Messaging section title | IM Channel | 消息渠道 |
| Messaging section description | Finish credentials and QR login for selected IM plugins. | 填写聊天工具需要的 token、密钥或扫码登录。 |
| Message detail section | IM message detail | 消息显示 |
| QR login title | QR Login | 二维码登录 |
| QR login description | Generate a QR code, scan it with the app, then wait for authorization. | 生成二维码，用对应 App 扫描，然后等待授权。 |
| QR scan hint | Scan with the app and confirm on your phone. | 用 App 扫码，并在手机上确认。 |
| Remote section title | Remote access configuration | 远程访问配置 |
| Remote section description | Paste tunnel details when remote access was selected. | 如果选择了远程访问，在这里填写 Cloudflare 或 ngrok 信息。 |
| Tunnel section title | Tunnel | 隧道 |
| Tunnel section description | Expose your local server to the internet for IM webhooks and remote access. Skip if you only use it locally. | 将本地服务暴露到互联网，用于 IM webhook 和远程访问。如果只本地使用，可以跳过。 |
| Empty state | No extra configuration | 没有额外配置 |
| Empty state description | The selected setup can launch now. | 现在可以启动 VibeAround。 |

说明：profile 名称、provider 名称、插件配置字段、远程访问 provider 名称来自动态配置。

## 请输出

请按下面格式输出优化结果：

1. 先给出整体文案策略，最多 5 条。
2. 再按 Shared Guidance、Step 1、Step 2、Step 3、Step 4、Step 5 输出表格。
3. 每张表格包含：
   - Placement
   - Revised English
   - Revised Chinese
   - Why
4. 如果某条文案你建议保持不变，也请照样列出，并在 Why 里说明。
5. 最后列出 5 条最重要的改动理由。

不要输出按钮文案，不要输出安装状态文案，不要输出代码。
```
