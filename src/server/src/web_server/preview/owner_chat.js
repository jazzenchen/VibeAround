(function () {
  "use strict";

  var picker = document.getElementById("preview-picker");
  var toggle = document.getElementById("preview-chat-toggle");
  var drawer = document.getElementById("preview-chat-drawer");
  var close = document.getElementById("preview-chat-close");
  var status = document.getElementById("preview-chat-status");
  var attention = document.getElementById("preview-chat-attention");
  var log = document.getElementById("preview-chat-log");
  var permissions = document.getElementById("preview-chat-permissions");
  var form = document.getElementById("preview-chat-form");
  var input = document.getElementById("preview-chat-input");
  var send = document.getElementById("preview-chat-send");
  var stop = document.getElementById("preview-chat-stop");
  var socket = null;
  var reconnectTimer = null;
  var reconnectAttempt = 0;
  var generation = 0;
  var active = false;
  var messageNodes = new Map();
  var currentAssistant = null;

  function selectedOption() {
    return picker.options[picker.selectedIndex];
  }

  function drawerOpen() {
    return drawer.dataset.open === "true";
  }

  function setDrawer(open) {
    drawer.dataset.open = open ? "true" : "false";
    drawer.setAttribute("aria-hidden", open ? "false" : "true");
    toggle.setAttribute("aria-expanded", open ? "true" : "false");
    if (open) {
      attention.hidden = true;
      input.focus();
      scrollToEnd();
    } else {
      toggle.focus();
    }
  }

  function setStatus(text) {
    status.textContent = text;
  }

  function setActive(next) {
    active = next;
    send.disabled = next || !socket || socket.readyState !== WebSocket.OPEN;
    stop.hidden = !next;
  }

  function clearConversation() {
    log.replaceChildren();
    permissions.replaceChildren();
    messageNodes.clear();
    currentAssistant = null;
    attention.hidden = true;
    setActive(false);
  }

  function emptyMessage(text) {
    var node = document.createElement("p");
    node.className = "chat-empty";
    node.textContent = text;
    log.appendChild(node);
  }

  function removeEmptyMessage() {
    var empty = log.querySelector(".chat-empty");
    if (empty) empty.remove();
  }

  function appendMessage(role, text, key) {
    if (!text) return null;
    removeEmptyMessage();
    var existing = key && messageNodes.get(key);
    if (existing) {
      if (role === "assistant") existing.textContent += text;
      return existing;
    }
    var row = document.createElement("div");
    var bubble = document.createElement("div");
    row.className = "chat-message";
    row.dataset.role = role;
    bubble.className = "chat-bubble";
    bubble.textContent = text;
    row.appendChild(bubble);
    log.appendChild(row);
    if (key) messageNodes.set(key, bubble);
    if (role === "assistant") currentAssistant = bubble;
    scrollToEnd();
    return bubble;
  }

  function scrollToEnd() {
    log.scrollTop = log.scrollHeight;
  }

  function contentText(content) {
    if (typeof content === "string") return content;
    if (Array.isArray(content)) return content.map(contentText).join("");
    if (!content || typeof content !== "object") return "";
    if (content.type === "text" && typeof content.text === "string") return content.text;
    return "";
  }

  function handleAcp(payload) {
    var update = payload && payload.update;
    if (!update || typeof update !== "object") return;
    var kind = update.sessionUpdate;
    if (kind === "user_message_chunk") {
      var userText = contentText(update.content);
      var userKey = update.messageId ? "user:" + update.messageId : null;
      if (!userKey || !messageNodes.has(userKey)) appendMessage("user", userText, userKey);
      currentAssistant = null;
      return;
    }
    if (kind === "agent_message_chunk") {
      var agentText = contentText(update.content);
      var agentKey = update.messageId ? "agent:" + update.messageId : null;
      if (agentKey) appendMessage("assistant", agentText, agentKey);
      else if (currentAssistant) {
        currentAssistant.textContent += agentText;
        scrollToEnd();
      } else appendMessage("assistant", agentText, null);
      return;
    }
    if (kind === "agent_thought_chunk") {
      setStatus("Thinking…");
      return;
    }
    if (kind === "tool_call" || kind === "tool_call_update") {
      var label = update.title || (update.toolCall && update.toolCall.title) || "tool";
      setStatus(update.status === "completed" ? "Working…" : "Using " + label + "…");
    }
  }

  function permissionTitle(request) {
    var tool = request && request.toolCall;
    return (tool && (tool.title || tool.kind)) || "Permission requested";
  }

  function renderPermission(frame) {
    var card = document.createElement("section");
    var title = document.createElement("strong");
    var actions = document.createElement("div");
    var options = frame.request && Array.isArray(frame.request.options) ? frame.request.options : [];
    card.className = "permission-card";
    actions.className = "permission-actions";
    title.textContent = permissionTitle(frame.request);
    card.appendChild(title);
    card.appendChild(actions);
    options.forEach(function (option) {
      if (!option || typeof option.optionId !== "string") return;
      var button = document.createElement("button");
      button.type = "button";
      button.textContent = option.name || option.optionId;
      button.addEventListener("click", function () {
        if (sendFrame({ type: "permission_response", requestId: frame.request_id, optionId: option.optionId })) {
          card.remove();
        }
      });
      actions.appendChild(button);
    });
    var cancel = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "Cancel";
    cancel.addEventListener("click", function () {
      if (sendFrame({ type: "permission_response", requestId: frame.request_id, outcome: "cancelled" })) {
        card.remove();
      }
    });
    actions.appendChild(cancel);
    permissions.appendChild(card);
    if (!drawerOpen()) attention.hidden = false;
  }

  function handleFrame(frame) {
    if (!frame || typeof frame.kind !== "string") return;
    if (frame.kind === "acp_notification") handleAcp(frame.payload);
    else if (frame.kind === "system_text") appendMessage("system", frame.text);
    else if (frame.kind === "error") appendMessage("error", frame.error);
    else if (frame.kind === "permission_request") renderPermission(frame);
    else if (frame.kind === "agent_ready") setStatus(frame.agent || "Connected");
    else if (frame.kind === "session_ready") setStatus(active ? "Working…" : "Connected");
    else if (frame.kind === "turn_status") {
      setActive(Boolean(frame.active));
      setStatus(frame.active ? "Working…" : "Connected");
      if (!frame.active) {
        currentAssistant = null;
        permissions.replaceChildren();
        attention.hidden = true;
      }
    }
  }

  function sendFrame(frame) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return false;
    socket.send(JSON.stringify(frame));
    return true;
  }

  function socketUrl(slug) {
    var scheme = location.protocol === "https:" ? "wss:" : "ws:";
    return scheme + "//" + location.host + "/va/preview/u/" + encodeURIComponent(slug) + "/chat";
  }

  function closeSocket() {
    generation += 1;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
    var previous = socket;
    socket = null;
    if (previous) previous.close();
  }

  function connect() {
    closeSocket();
    clearConversation();
    var option = selectedOption();
    if (!option || option.dataset.chatAvailable !== "true") {
      setStatus("Unavailable");
      emptyMessage("This Preview is not linked to an AI task. Recreate it from the current task.");
      return;
    }
    var localGeneration = generation;
    setStatus("Connecting…");
    emptyMessage("No messages yet.");
    try {
      socket = new WebSocket(socketUrl(option.value));
    } catch (_) {
      scheduleReconnect(localGeneration);
      return;
    }
    var current = socket;
    current.addEventListener("open", function () {
      if (socket !== current || generation !== localGeneration) return;
      reconnectAttempt = 0;
      setStatus("Connected");
      setActive(false);
    });
    current.addEventListener("message", function (event) {
      if (socket !== current || typeof event.data !== "string") return;
      try { handleFrame(JSON.parse(event.data)); } catch (_) { return; }
    });
    current.addEventListener("close", function () {
      if (socket !== current || generation !== localGeneration) return;
      socket = null;
      setActive(false);
      setStatus("Reconnecting…");
      scheduleReconnect(localGeneration);
    });
    current.addEventListener("error", function () {
      if (socket === current) setStatus("Connection error");
    });
  }

  function scheduleReconnect(localGeneration) {
    if (generation !== localGeneration || reconnectTimer) return;
    var delays = [1000, 2000, 5000, 10000];
    var delay = delays[Math.min(reconnectAttempt, delays.length - 1)];
    reconnectAttempt += 1;
    reconnectTimer = setTimeout(function () {
      reconnectTimer = null;
      if (generation === localGeneration) connect();
    }, delay);
  }

  toggle.addEventListener("click", function () { setDrawer(!drawerOpen()); });
  close.addEventListener("click", function () { setDrawer(false); });
  picker.addEventListener("change", function () {
    reconnectAttempt = 0;
    connect();
  });
  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape" && drawerOpen()) setDrawer(false);
  });
  form.addEventListener("submit", function (event) {
    event.preventDefault();
    var text = input.value.trim();
    if (!text || active) return;
    var messageId = typeof crypto.randomUUID === "function" ? crypto.randomUUID() : String(Date.now());
    if (sendFrame({ type: "message", messageId: messageId, text: text })) {
      appendMessage("user", text, "user:" + messageId);
      input.value = "";
      setActive(true);
      setStatus("Working…");
    }
  });
  input.addEventListener("keydown", function (event) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      form.requestSubmit();
    }
  });
  stop.addEventListener("click", function () {
    if (sendFrame({ type: "stop" })) setStatus("Stopping…");
  });

  connect();
})();
