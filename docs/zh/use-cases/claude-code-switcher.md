# Claude Code 供应商切换

场景是这样的：你想运行 Claude Code，但走你自己选的供应商和 key —— DeepSeek、Kimi、GLM、OpenRouter 或自托管的兼容端点 —— 不想每次开工前改环境变量，也不需要订阅官方编程套餐。VibeAround 的供应商 Profile 加 API Bridge 把这种路由变成启动时一个有名字、看得见的选择。

Claude 和 Claude Code 是 Anthropic 的产品。VibeAround 是协调本地工作流的独立软件。

## 完整走一遍

1. **创建供应商 Profile。** 在桌面应用的模型 Profile 页面：
   - 从目录选供应商（Kimi/Moonshot、DeepSeek、OpenRouter、Z.AI/GLM、DashScope 等 —— 端点和模型已预填），或选 **custom** 接任何兼容端点。
   - 同一供应商有多个端点变体时选对（全球 vs 国内、按量付费 vs 编程套餐）—— key 必须和套餐匹配。
   - 粘贴 API key。它只存本地，只由守护进程发给该供应商。
   - 选 Profile 暴露哪些模型、默认哪个。对校验模型名的 Agent，定义映射到真实上游模型的别名模型 id。

   完整细节见[模型 Profile 指南](../guides/model-profiles.md)；CLI 用 `va profiles` 列出。
2. **带着 Profile 启动 Claude Code。** 桌面 **Launch** 页面：选 Claude Code + Workspace + 新 Profile —— 或从 CLI：

   ```bash
   va launch --profile <name>            # 用保存的启动配置启动
   va launch --profile <name> --dry-run  # 打印渲染出的计划，不实际启动
   ```

   渲染出的配置把 Claude Code 的模型流量指向本地 Bridge（`127.0.0.1:12358`）；守护进程负责翻译到供应商。会话存续期间保持守护进程运行。
3. **先用小任务验证**这条路由再依赖它。Agent 说"找不到模型"时，用 Profile 的别名模型 id，不要用上游 id。
4. **托管对话中途切换。** 在 IM 或 Web Chat 的 Thread 里，`/profile --list` 列出 Profile，`/profile --switch <id>` 重新绑定 —— 拿同一个任务对比不同供应商很方便。

## 什么时候不切

原生的 Claude Code 配置（你的 Anthropic 登录）已经是最简单、最可靠的选择时，不要叠加 Profile 层。供应商切换的价值在于可重复性、显式路由、模型对比、第三方 key 或 Bridge 转换。

## 同一个切换，处处可用

Profile 不是 Claude Code 专属。同一个 Profile 可以启动 Codex CLI、Gemini CLI、OpenCode 和其他受支持的 Agent；任何 OpenAI 兼容工具都可以指向 Profile 的本地端点（Profile UI 里有显示），共享同一份供应商配置。Agent、Workspace、Session、Profile 都是按启动选择的 —— 看得见、可回退、可重复。

## 相关文档

- [模型 Profile 指南](../guides/model-profiles.md)
- [供应商端点参考](../reference/provider-endpoints.md)
- [Claude Code 远程访问](claude-remote.md)
- [本地 API 与 Bridge](../architecture/local-api-and-bridge.md)

---

*Last verified: v0.7.11*

<sub>[文档索引](../README.md)</sub>
