(function () {
  "use strict";

  var log = document.getElementById("preview-chat-log");
  var messageNodes = new Map();
  var currentAssistant = null;

  function scrollToEnd() {
    log.scrollTop = log.scrollHeight;
  }

  function removeEmpty() {
    var empty = log.querySelector(".chat-empty");
    if (empty) empty.remove();
  }

  function append(role, text, key) {
    if (!text) return null;
    removeEmpty();
    var existing = key && messageNodes.get(key);
    if (existing) {
      if (role === "assistant") existing.textContent += text;
      scrollToEnd();
      return existing;
    }
    var row = document.createElement("div");
    var content = document.createElement("div");
    row.className = "chat-message";
    row.dataset.role = role;
    content.className = "chat-message-content";
    content.textContent = text;
    row.appendChild(content);
    log.appendChild(row);
    if (key) messageNodes.set(key, content);
    if (role === "assistant") currentAssistant = content;
    scrollToEnd();
    return content;
  }

  function contentText(content) {
    if (typeof content === "string") return content;
    if (Array.isArray(content)) return content.map(contentText).join("");
    if (!content || typeof content !== "object") return "";
    return content.type === "text" && typeof content.text === "string" ? content.text : "";
  }

  function handleAcp(payload, setStatus) {
    var update = payload && payload.update;
    if (!update || typeof update !== "object") return;
    var kind = update.sessionUpdate;
    if (kind === "user_message_chunk") {
      var userKey = update.messageId ? "user:" + update.messageId : null;
      if (!userKey || !messageNodes.has(userKey)) append("user", contentText(update.content), userKey);
      currentAssistant = null;
    } else if (kind === "agent_message_chunk") {
      var text = contentText(update.content);
      var agentKey = update.messageId ? "agent:" + update.messageId : null;
      if (agentKey) append("assistant", text, agentKey);
      else if (currentAssistant) {
        currentAssistant.textContent += text;
        scrollToEnd();
      } else append("assistant", text, null);
    } else if (kind === "agent_thought_chunk") {
      setStatus("Thinking…");
    } else if (kind === "tool_call" || kind === "tool_call_update") {
      var label = update.title || update.toolCall && update.toolCall.title || "tool";
      setStatus(update.status === "completed" ? "Working…" : "Using " + label + "…");
    }
  }

  window.VAPreviewTranscript = Object.freeze({
    append: append,
    clear: function () {
      log.replaceChildren();
      messageNodes.clear();
      currentAssistant = null;
    },
    empty: function (text) {
      var node = document.createElement("p");
      node.className = "chat-empty";
      node.textContent = text;
      log.appendChild(node);
    },
    finishTurn: function () { currentAssistant = null; },
    handleAcp: handleAcp,
    scrollToEnd: scrollToEnd,
  });
})();
