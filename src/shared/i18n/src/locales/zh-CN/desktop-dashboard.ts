export const zhCNDesktopDashboard: Record<string, string> = {
  Running: "运行中",
  Spawning: "启动中",
  "Not started": "未启动",
  Stopped: "已停止",
  Crashed: "已崩溃",
  Failed: "失败",
  Busy: "忙碌",
  Idle: "空闲",
  Status: "状态",
  Tunnel: "隧道",
  Agents: "Agent",
  Channels: "频道",
  Launch: "启动",
  Previews: "预览",
  Workspaces: "工作区",
  Live: "实时",
  Polling: "轮询中",
  "Server failed to start": "服务器启动失败",
  Retry: "重试",
  "Open Config Wizard": "打开配置向导",
  "Open Web Dashboard": "打开 Web 控制台",
  "Save & Restart Services": "保存并重启服务",
  "Restarting services…": "正在重启服务…",
  "Settings saved.": "设置已保存。",
  "Settings applied.": "设置已应用。",
  "Services restarted.": "服务已重启。",
  General: "通用",
  IM: "IM",
  "IM Channel": "通讯工具",
  Sessions: "会话",
  Proxy: "代理",
  "Restart Services": "重启服务",
  Restart: "重启",
  "Rerun Onboarding": "重新运行配置向导",
  "Manage local service controls and rerun setup when needed.":
    "管理本地服务，并在需要时重新运行配置向导。",
  "Restart VibeAround runtime services after local changes.":
    "本地配置修改后重启 VibeAround 运行服务。",
  "Open the configuration wizard again.": "重新打开配置向导。",
  "Apply IM Channel Settings": "应用通讯工具设置",
  "Apply Session Settings": "应用会话设置",
  "Apply Agent Settings": "应用 Agent 设置",
  "Apply Proxy Settings": "应用代理设置",
  "Applying…": "应用中…",
  "Agent settings applied.": "Agent 设置已应用。",
  "IM Channel settings applied.": "通讯工具设置已应用。",
  "Session settings applied.": "会话设置已应用。",
  "Proxy settings applied.": "代理设置已应用。",
  "Tunnel settings saved.": "隧道设置已保存。",
  "Tunnel settings applied.": "隧道设置已应用。",
  "API bridge retry": "API 转接重试",
  "Automatically retry upstream requests that return 429.":
    "上游返回 429 时自动等待后重试。",
  "Auto retry 429": "自动重试 429",
  "Retry upstream API requests when the provider reports rate limiting.":
    "当服务商返回限流时，自动重试上游 API 请求。",
  "Max retries": "最大重试次数",
  "Set to unlimited to keep waiting through provider throttling.":
    "设为无限后，会一直等待服务商限流解除。",
  "Retry indefinitely": "无限重试",
  "Delay seconds": "延迟秒数",
  "Used between 429 retries unless upstream sends Retry-After.":
    "如果上游没有返回 Retry-After，每次 429 重试前等待这个秒数。",
  "Choose which CLIs appear in Launch and new IM sessions. Running sessions continue.":
    "选择哪些 CLI 出现在启动页和新的 IM 会话中。已运行的会话不会受影响。",
  "No agents are enabled. Launch will stay hidden until at least one agent is selected.":
    "当前没有启用 Agent。至少选择一个 Agent 后，启动入口才会显示。",
  "Auto-install MCP": "自动安装 MCP",
  "Install VibeAround MCP in the selected workspace when an agent launches.":
    "Agent 启动时，在所选工作区安装 VibeAround MCP。",
  "Auto-install skills": "自动安装 skill",
  "Install VibeAround skills in the selected workspace when an agent launches.":
    "Agent 启动时，在所选工作区安装 VibeAround skill。",
  "Uninstall legacy MCP": "卸载旧版 MCP",
  "Remove legacy VibeAround MCP entries from old global config.":
    "从旧的全局配置中移除 VibeAround MCP 条目。",
  "Uninstall legacy skill": "卸载旧版 skill",
  "Remove legacy VibeAround skill files from old global folders.":
    "从旧的全局目录中移除 VibeAround skill 文件。",
  "Legacy VibeAround MCP entries removed.":
    "旧版 VibeAround MCP 条目已移除。",
  "Legacy VibeAround skill files removed.":
    "旧版 VibeAround skill 文件已移除。",
  "Configure the HTTP proxy used by profile provider requests that opt in from profile settings.":
    "配置服务商请求使用的 HTTP 代理，可在服务商配置中启用。",
  "Enable HTTP proxy": "启用 HTTP 代理",
  "Allow profiles to opt in to this HTTP proxy.":
    "允许在服务商配置中启用 HTTP 代理。",
  "HTTP proxy URL": "HTTP 代理 URL",
  "Proxy bypass list": "代理排除列表",
  "Comma-separated hosts, domains, or IPs that should connect directly.":
    "用逗号分隔，匹配的主机、域名或 IP 将直连。",
  "Configure how VibeAround restores active conversations.":
    "配置 VibeAround 如何恢复进行中的对话。",
  "Auto-continue IM Channel sessions": "自动延续通讯工具会话",
  "When an IM Channel message attaches to a thread, continue that thread's latest agent session without replaying old output.":
    "当通讯工具消息附加到 thread 时，延续该 thread 最近的 agent session，并且不回放旧输出。",
  "Runtime health for tunnels, agents, and messaging channels.":
    "查看隧道、Agent 和消息频道的运行状态。",
  "No tunnel running": "没有运行中的隧道",
  "No agents running": "没有运行中的 Agent",
  "No channels running": "没有运行中的频道",
  "retry {{seconds}}s": "{{seconds}} 秒后重试",
  "Tunnel ({{provider}})": "隧道（{{provider}}）",
  "Stopped {{name}}": "已停止 {{name}}",
  "Restarting {{name}}": "正在重启 {{name}}",
  Update: "有更新",
  "Update to VibeAround {{version}}": "更新到 VibeAround {{version}}",
};
