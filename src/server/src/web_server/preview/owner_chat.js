(function () {
  "use strict";

  var picker = document.getElementById("preview-picker");
  var toggle = document.getElementById("preview-chat-toggle");
  var drawer = document.getElementById("preview-chat-drawer");
  var close = document.getElementById("preview-chat-close");
  var status = document.getElementById("preview-chat-status");
  var attention = document.getElementById("preview-chat-attention");
  var permissions = document.getElementById("preview-chat-permissions");
  var form = document.getElementById("preview-chat-form");
  var input = document.getElementById("preview-chat-input");
  var action = document.getElementById("preview-chat-action");
  var sendIcon = document.getElementById("preview-chat-send-icon");
  var stopIcon = document.getElementById("preview-chat-stop-icon");
  var socket = null;
  var reconnectTimer = null;
  var reconnectAttempt = 0;
  var generation = 0;
  var active = false;
  var transcript = window.VAPreviewTranscript;

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
      transcript.scrollToEnd();
    } else {
      toggle.focus();
    }
  }

  function setStatus(text) {
    status.textContent = text;
  }

  function setActive(next) {
    active = next;
    action.type = next ? "button" : "submit";
    action.disabled = !next && (!socket || socket.readyState !== WebSocket.OPEN);
    action.classList.toggle("primary-button", !next);
    action.setAttribute("aria-label", next ? "Stop" : "Send");
    action.title = next ? "Stop" : "Send";
    sendIcon.toggleAttribute("hidden", next);
    stopIcon.toggleAttribute("hidden", !next);
  }

  function resizeInput() {
    input.style.height = "auto";
    var height = Math.max(72, Math.min(256, input.scrollHeight));
    input.style.height = height + "px";
    input.style.overflowY = input.scrollHeight > 256 ? "auto" : "hidden";
  }

  function clearConversation() {
    transcript.clear();
    permissions.replaceChildren();
    attention.hidden = true;
    setActive(false);
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
    if (frame.kind === "acp_notification") transcript.handleAcp(frame.payload, active);
    else if (frame.kind === "system_text") transcript.append("system", frame.text);
    else if (frame.kind === "error") transcript.append("error", frame.error);
    else if (frame.kind === "permission_request") renderPermission(frame);
    else if (frame.kind === "agent_ready" || frame.kind === "session_ready") setStatus("Connected");
    else if (frame.kind === "turn_status") {
      var wasActive = active;
      setActive(Boolean(frame.active));
      if (frame.active && !wasActive) transcript.beginTurn();
      if (!frame.active) {
        transcript.finishTurn();
        permissions.replaceChildren();
        attention.hidden = true;
        if (wasActive) document.dispatchEvent(new CustomEvent("va-preview-turn-complete"));
      }
    }
  }

  function sendFrame(frame) {
    if (!socket || socket.readyState !== WebSocket.OPEN) return false;
    socket.send(JSON.stringify(frame));
    return true;
  }

  function canSend() {
    return !active && socket && socket.readyState === WebSocket.OPEN;
  }

  function sendMessage(text, displayText) {
    var message = String(text || "").trim();
    if (!message || !canSend()) return false;
    var messageId = typeof crypto.randomUUID === "function" ? crypto.randomUUID() : String(Date.now());
    if (!sendFrame({ type: "message", messageId: messageId, text: message })) return false;
    transcript.append("user", displayText || message, "user:" + messageId);
    setActive(true);
    transcript.beginTurn();
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
      transcript.empty("This Preview is not linked to an AI task. Recreate it from the current task.");
      return;
    }
    var localGeneration = generation;
    setStatus("Connecting…");
    transcript.empty("No messages yet.");
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
    if (sendMessage(text)) {
      input.value = "";
      resizeInput();
    }
  });
  input.addEventListener("input", resizeInput);
  input.addEventListener("keydown", function (event) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      form.requestSubmit();
    }
  });
  action.addEventListener("click", function (event) {
    if (!active) return;
    event.preventDefault();
    if (sendFrame({ type: "stop" })) transcript.setActivity("Stopping…");
  });

  window.VAPreviewChat = Object.freeze({ canSend: canSend, send: sendMessage });

  resizeInput();
  connect();
})();
