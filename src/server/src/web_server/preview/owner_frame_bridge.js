(function () {
  "use strict";

  var scope = "va-preview-review";
  var version = 1;
  var picker = document.getElementById("preview-picker");
  var frame = document.getElementById("preview-frame");
  var channelId = createChannelId();
  var ready = false;

  function createChannelId() {
    return typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : Date.now().toString(36) + "-" + Math.random().toString(36).slice(2);
  }

  function frameUrl() {
    try { return new URL(frame.src, location.href); } catch (_) { return null; }
  }

  function reviewAvailable() {
    var option = picker.options[picker.selectedIndex];
    return Boolean(option && option.dataset.chatAvailable === "true");
  }

  function comments(method, args) {
    var api = window.VAPreviewComments;
    if (api && typeof api[method] === "function") api[method].apply(api, args || []);
  }

  function post(type, fields) {
    var url = frameUrl();
    if (!url || !frame.contentWindow) return false;
    frame.contentWindow.postMessage(Object.assign({
      scope: scope,
      version: version,
      channelId: channelId,
      type: type,
    }, fields || {}), url.origin);
    return true;
  }

  function command(type, fields) {
    return ready && post(type, fields);
  }

  function resetFrame() {
    ready = false;
    comments("resetFrame");
    if (reviewAvailable()) post("init");
    else comments("setCapabilities", [[]]);
  }

  function validEvent(event) {
    var url = frameUrl();
    return Boolean(url
      && event.source === frame.contentWindow
      && event.origin === url.origin
      && event.data
      && event.data.scope === scope
      && event.data.version === version);
  }

  window.addEventListener("message", function (event) {
    if (!validEvent(event)) return;
    var message = event.data;
    if (message.type === "hello") {
      if (reviewAvailable()) post("init");
      return;
    }
    if (message.channelId !== channelId) return;
    if (message.type === "ready") {
      ready = true;
      comments("setCapabilities", [message.capabilities]);
    } else if (message.type === "anchor-picked") {
      comments("pick", [message.anchor, message.rect, message.selectionId]);
    } else if (message.type === "marker-activate") {
      comments("activate", [message.markerId, message.rect]);
    } else if (message.type === "cancel") {
      comments("cancelPick");
    }
  });

  picker.addEventListener("change", resetFrame);
  frame.addEventListener("load", resetFrame);

  window.VAPreviewFrameReview = Object.freeze({
    setMarker: function (id, _anchor, selectionId) {
      return command("set-marker", { markerId: id, selectionId: selectionId });
    },
    removeMarker: function (id) {
      return command("remove-marker", { markerId: id });
    },
    focusMarker: function (id) {
      return command("focus-marker", { markerId: id });
    },
    setElementMode: function (enabled) {
      return command("element-mode", { enabled: Boolean(enabled) });
    },
    cancelPick: function () { return command("cancel"); },
  });

  resetFrame();
})();
