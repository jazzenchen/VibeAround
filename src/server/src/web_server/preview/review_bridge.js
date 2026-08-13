(function () {
  "use strict";
  const script = document.currentScript;
  if (!script || !script.src || window.parent === window) return;
  const scope = "va-preview-review";
  const version = 1;
  const documentId = typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `preview-document-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const scriptUrl = new URL(script.src, location.href);
  const allowedOwnerOrigins = new Set([scriptUrl.origin]);
  if (["127.0.0.1", "localhost", "[::1]"].includes(scriptUrl.hostname)) {
    const port = scriptUrl.port ? `:${scriptUrl.port}` : "";
    for (const host of ["127.0.0.1", "localhost", "[::1]"]) {
      allowedOwnerOrigins.add(`${scriptUrl.protocol}//${host}${port}`);
    }
  }
  let ownerOrigin = scriptUrl.origin;
  const markdownSource = document.getElementById("markdown-source");
  const selections = new Map();
  const markers = new Map();
  const blockedText = "input,textarea,select,[contenteditable]:not([contenteditable='false'])";
  let channelId = null;
  let sequence = 0;
  let pickMode = "none";
  let pending = null;
  let activeAnchor = null;
  let overlayFrame = 0;
  let regionPicker = null;
  let host, root, hover, hoverLabel, trigger;

  function post(type, fields) {
    parent.postMessage({ scope, version, type, channelId, ...fields }, ownerOrigin);
  }
  const clipped = (value, limit) => String(value || "").replace(/\s+/g, " ").trim().slice(0, limit);
  const elementFor = (node) => node && (node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement);
  const page = () => ({ path: location.pathname, hash: location.hash, title: clipped(document.title, 240) });
  function safeText(element, limit) {
    if (!element || element.matches(blockedText)) return "";
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    let value = "";
    for (let node = walker.nextNode(); node && value.length < limit * 2; node = walker.nextNode()) {
      const owner = elementFor(node);
      if (!owner.closest(blockedText)) value += ` ${node.data}`;
    }
    return clipped(value, limit);
  }
  function headingFor(node) {
    const element = elementFor(node);
    if (!element) return "";
    const own = element.closest("h1,h2,h3,h4,h5,h6");
    if (own) return safeText(own, 240);
    let heading = "";
    for (const candidate of document.querySelectorAll("h1,h2,h3,h4,h5,h6")) {
      if (!(candidate.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING)) break;
      heading = safeText(candidate, 240);
    }
    return heading;
  }
  function markdownLines(exact) {
    if (!markdownSource || !exact) return null;
    try {
      const markdown = JSON.parse(markdownSource.textContent);
      const start = markdown.indexOf(exact);
      if (start < 0 || markdown.indexOf(exact, start + 1) >= 0) return null;
      const startLine = markdown.slice(0, start).split("\n").length;
      return { startLine, endLine: startLine + (exact.match(/\n/g) || []).length };
    } catch (_) { return null; }
  }
  function elementSummary(element) {
    const summary = { tag: element.tagName.toLowerCase() };
    for (const [attribute, key] of [["id", "id"], ["data-testid", "testId"],
      ["role", "role"], ["aria-label", "label"]]) {
      const value = clipped(element.getAttribute(attribute), 240);
      if (value) summary[key] = value;
    }
    if (summary.id) summary.selector = `#${CSS.escape(summary.id)}`;
    else if (summary.testId) summary.selector = `[data-testid="${CSS.escape(summary.testId)}"]`;
    const text = safeText(element, 320);
    if (text) summary.text = text;
    return summary;
  }
  function makeAnchor(kind, target, exact) {
    const text = kind === "text"
      ? String(exact || "").trim().slice(0, 2000) : safeText(target, 320);
    const result = { kind, text, heading: headingFor(target) };
    if (!markdownSource) result.page = page();
    if (kind === "element" || !markdownSource) result.element = elementSummary(elementFor(target));
    const lines = markdownLines(String(exact || text).trim());
    return lines ? { ...result, ...lines } : result;
  }
  function editableSelection(selection) {
    return [selection.anchorNode, selection.focusNode].some((node) => {
      const element = elementFor(node);
      return element && element.closest(blockedText);
    });
  }
  function ensureUi() {
    if (host) return;
    host = document.createElement("div");
    host.style.cssText = "position:fixed;inset:0;z-index:2147483647;pointer-events:none";
    root = host.attachShadow({ mode: "closed" });
    host.dataset.html2canvasIgnore = "true";
    root.innerHTML = `<style>.box{position:fixed;border:2px solid #0d9488;background:#14b8a61f;border-radius:4px;pointer-events:none}
      .hover{border-style:dashed;background:#14b8a612}.hover-label{position:fixed;max-width:320px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;border-radius:4px;background:#0f766e;color:#fff;padding:3px 7px;font:600 11px system-ui;pointer-events:none}button{position:fixed;border:1px solid #99f6e4;border-radius:999px;background:#fff;color:#134e4a;box-shadow:0 2px 8px #0003;font:600 12px system-ui;pointer-events:auto;cursor:pointer}button[hidden],.hover-label[hidden]{display:none}.trigger{padding:6px 9px}.marker{display:grid;place-items:center;width:26px;height:26px;padding:0}.marker svg{width:14px;height:14px;fill:none;stroke:currentColor;stroke-width:2;stroke-linecap:round;stroke-linejoin:round}.region-pin::after{content:"";position:absolute;top:50%;width:14px;height:1px;background:#0d9488}.region-pin.pin-left::after{left:100%}.region-pin.pin-right::after{right:100%}.focus{outline:3px solid #0d948859;outline-offset:2px}
      .region{position:fixed;inset:0;pointer-events:auto;cursor:crosshair;touch-action:none;background:#14b8a60a}.region[hidden],.region-box[hidden],.region-status[hidden]{display:none}.region-box{position:fixed;border:2px dashed #0d9488;background:#14b8a61a;pointer-events:none}.region-status{position:fixed;left:50%;top:50%;translate:-50% -50%;border-radius:999px;background:#fff;color:#134e4a;box-shadow:0 4px 18px #0003;padding:8px 12px;font:600 12px system-ui;pointer-events:none}</style>
      <div class="box hover" hidden></div><div class="hover-label" hidden></div><button class="trigger" type="button" hidden>Comment</button>
      <div class="region" hidden><div class="region-box" hidden></div><div class="region-status" hidden>Capturing screenshot…</div></div>`;
    hover = root.querySelector(".hover");
    hoverLabel = root.querySelector(".hover-label");
    trigger = root.querySelector(".trigger");
    trigger.addEventListener("pointerdown", (event) => event.preventDefault());
    trigger.addEventListener("click", (event) => {
      event.stopPropagation();
      if (pending) {
        pending.message.rect = messageRect(pending.selection);
        activeAnchor = { id: pending.selectionId, selection: pending.selection };
        post("anchor-picked", pending.message);
      }
      trigger.hidden = true;
    });
    regionPicker = createPreviewRegionPicker({
      region: root.querySelector(".region"),
      regionBox: root.querySelector(".region-box"),
      regionStatus: root.querySelector(".region-status"),
      host,
      renderer: vaPreviewHtml2canvas,
      onPicked: ({ rect, target, screenshot, documentPoint }) => {
        pickMode = "none";
        setPending(newRegionSelection(rect, target, screenshot, documentPoint), false);
        activeAnchor = { id: pending.selectionId, selection: pending.selection };
        post("anchor-picked", pending.message);
      },
      onError: (message) => {
        pickMode = "none";
        post("capture-error", { message });
      },
      onCancel: () => {
        pickMode = "none";
        if (channelId) post("cancel");
      },
    });
    document.documentElement.appendChild(host);
  }
  function hiddenByClosedDetails(element) {
    for (let ancestor = element; ancestor; ancestor = ancestor.parentElement) {
      if (ancestor.tagName !== "DETAILS" || ancestor.open) continue;
      const summary = ancestor.querySelector(":scope > summary");
      if (ancestor !== element && (!summary || !summary.contains(element))) return true;
    }
    return false;
  }
  function rectFor(selection) {
    if (selection.region) {
      const saved = selection.region;
      const left = saved.documentX - scrollX;
      const top = saved.documentY - scrollY;
      return {
        x: left, y: top, left, top,
        right: left + saved.width, bottom: top + saved.height,
        width: saved.width, height: saved.height,
      };
    }
    const owner = selection.element || elementFor(selection.range.commonAncestorContainer);
    if (!owner || !owner.isConnected || hiddenByClosedDetails(owner)) return null;
    return selection.element ? owner.getBoundingClientRect() : selection.range.getBoundingClientRect();
  }
  const messageRect = (selection) => {
    const rect = rectFor(selection); return rect && { x: rect.x, y: rect.y,
      width: rect.width, height: rect.height };
  };
  const hasArea = (rect) => rect && (rect.width || rect.height);
  const rectVisible = (rect) => rect && rect.bottom > 0 && rect.top < innerHeight
    && rect.right > 0 && rect.left < innerWidth;
  function place(node, rect, icon) {
    if (!hasArea(rect)) return void (node.hidden = true);
    node.hidden = false;
    node.style.left = `${icon ? Math.max(4, rect.right - 13) : rect.left}px`;
    node.style.top = `${icon ? Math.max(4, rect.top - 13) : rect.top}px`;
    if (!icon) Object.assign(node.style, {
      width: `${rect.width}px`, height: `${rect.height}px`,
    });
  }
  function placeRegionPin(node, rect) {
    if (!hasArea(rect) || !rectVisible(rect)) return void (node.hidden = true);
    const pinLeft = rect.left >= 44;
    node.hidden = false;
    node.classList.toggle("pin-left", pinLeft);
    node.classList.toggle("pin-right", !pinLeft);
    node.style.left = `${pinLeft
      ? Math.max(4, rect.left - 40)
      : Math.min(innerWidth - 30, rect.right + 14)}px`;
    node.style.top = `${Math.max(4, Math.min(innerHeight - 30, rect.top))}px`;
  }
  function updateOverlays() {
    if (pending && !trigger.hidden) {
      const rect = rectFor(pending.selection);
      if (!hasArea(rect)) trigger.hidden = true;
      else {
        trigger.style.left = `${Math.min(innerWidth - 88, Math.max(4, rect.right - 72))}px`;
        trigger.style.top = `${Math.min(innerHeight - 36, Math.max(4, rect.bottom + 6))}px`;
      }
    }
    for (const marker of markers.values()) {
      const rect = rectFor(marker.selection);
      if (marker.highlight) place(marker.highlight, rect, false);
      if (marker.regionPin) placeRegionPin(marker.button, rect);
      else place(marker.button, rect, true);
    }
    if (activeAnchor && channelId) {
      const rect = rectFor(activeAnchor.selection);
      const visible = rectVisible(rect);
      if (!hasArea(rect) || (!activeAnchor.activateWhenVisible && !visible)) {
        const anchorId = activeAnchor.id;
        activeAnchor = null;
        post("anchor-position", { anchorId, rect: null });
      } else if (visible) {
        const serialized = { x: rect.x, y: rect.y,
          width: rect.width, height: rect.height };
        if (activeAnchor.activateWhenVisible) {
          activeAnchor.activateWhenVisible = false;
          post("marker-activate", { markerId: activeAnchor.id, rect: serialized });
        }
        post("anchor-position", {
          anchorId: activeAnchor.id,
          rect: serialized,
        });
      }
    }
  }
  function scheduleOverlayUpdate() {
    if (overlayFrame) return;
    overlayFrame = requestAnimationFrame(() => {
      overlayFrame = 0;
      updateOverlays();
    });
  }
  function newSelection(kind, range, element, exact) {
    const selectionId = `selection-${++sequence}`;
    const selection = { range, element };
    const message = { selectionId, rect: messageRect(selection),
      anchor: makeAnchor(kind, element || range.commonAncestorContainer, exact) };
    selections.set(selectionId, selection);
    return { selectionId, selection, message };
  }
  function newRegionSelection(rect, target, screenshot, documentPoint) {
    const selectionId = `selection-${++sequence}`;
    const width = Math.round(rect.width);
    const height = Math.round(rect.height);
    const selection = {
      region: {
        documentX: documentPoint.x,
        documentY: documentPoint.y,
        width: rect.width,
        height: rect.height,
      },
      element: elementFor(target),
    };
    const anchor = {
      kind: "region",
      text: `Screenshot region ${width} × ${height}`,
      heading: headingFor(target),
      region: { width, height },
    };
    if (!markdownSource) anchor.page = page();
    const message = {
      selectionId,
      rect: messageRect(selection),
      anchor,
      screenshot,
    };
    selections.set(selectionId, selection);
    return { selectionId, selection, message };
  }
  function setPending(next, showTrigger) {
    if (pending) selections.delete(pending.selectionId);
    activeAnchor = null;
    pending = next;
    trigger.hidden = !showTrigger;
    updateOverlays();
  }
  function cancelPick(notify) {
    if (pending) selections.delete(pending.selectionId);
    pending = null; activeAnchor = null; pickMode = "none";
    if (hover) hover.hidden = true;
    if (hoverLabel) hoverLabel.hidden = true;
    if (trigger) trigger.hidden = true;
    if (regionPicker) regionPicker.cancel();
    if (notify && channelId) post("cancel");
  }
  function addMarker(markerId, selectionId) {
    const selection = selections.get(selectionId);
    if (!selection || !markerId) return;
    removeMarker(markerId);
    const regionPin = Boolean(selection.region);
    const markerSelection = regionPin ? { element: selection.element } : selection;
    if (regionPin && !markerSelection.element) {
      selections.delete(selectionId);
      if (pending && pending.selectionId === selectionId) pending = null;
      return;
    }
    const highlight = regionPin ? null : document.createElement("div");
    const button = document.createElement("button");
    const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    const iconPath = document.createElementNS("http://www.w3.org/2000/svg", "path");
    if (highlight) highlight.className = "box";
    Object.assign(button, {
      className: regionPin ? "marker region-pin" : "marker",
      type: "button",
    });
    button.setAttribute("aria-label", "Open comment");
    icon.setAttribute("viewBox", "0 0 24 24");
    icon.setAttribute("aria-hidden", "true");
    iconPath.setAttribute("d", "M21 15a4 4 0 0 1-4 4H8l-5 3V7a4 4 0 0 1 4-4h10a4 4 0 0 1 4 4z");
    icon.appendChild(iconPath);
    button.appendChild(icon);
    button.addEventListener("click", () => {
      if (pending) cancelPick(false);
      activeAnchor = { id: markerId, selection: markerSelection };
      post("marker-activate", { markerId, rect: messageRect(markerSelection) });
    });
    if (highlight) root.append(highlight);
    root.append(button);
    markers.set(markerId, { selection: markerSelection, highlight, button, regionPin });
    if (activeAnchor && activeAnchor.id === selectionId) activeAnchor = null;
    selections.delete(selectionId);
    if (pending && pending.selectionId === selectionId) pending = null;
    const browserSelection = getSelection(); if (browserSelection) browserSelection.removeAllRanges();
    updateOverlays();
  }
  function removeMarker(markerId) {
    const marker = markers.get(markerId);
    if (!marker) return;
    if (marker.highlight) marker.highlight.remove();
    marker.button.remove();
    markers.delete(markerId);
    if (activeAnchor && activeAnchor.id === markerId) activeAnchor = null;
  }
  function focusMarker(markerId) {
    const marker = markers.get(markerId);
    if (!marker) return;
    const rect = rectFor(marker.selection);
    if (!hasArea(rect)) return;
    activeAnchor = { id: markerId, selection: marker.selection, activateWhenVisible: true };
    const owner = marker.selection.element
      || elementFor(marker.selection.range.startContainer);
    owner.scrollIntoView({ behavior: "smooth", block: "center", inline: "nearest" });
    marker.button.classList.add("focus");
    setTimeout(() => marker.button.classList.remove("focus"), 1200);
    updateOverlays();
  }
  function captureTextSelection(event) {
    if (!channelId || pickMode !== "none" || (host && event.composedPath().includes(host))) return;
    const selection = getSelection();
    if (!selection || selection.isCollapsed || !selection.rangeCount || editableSelection(selection)) return;
    const exact = selection.toString().trim();
    if (!exact) return;
    ensureUi();
    setPending(newSelection("text", selection.getRangeAt(0).cloneRange(), null, exact), true);
  }
  document.addEventListener("pointerup", captureTextSelection, true);
  document.addEventListener("keyup", (event) => {
    if (event.shiftKey || ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a")) {
      captureTextSelection(event);
    }
  }, true);
  document.addEventListener("pointermove", (event) => {
    if (pickMode !== "element" || (host && event.composedPath().includes(host))) return;
    const element = elementFor(event.target);
    const target = element && element !== host ? element : null;
    const rect = target && target.getBoundingClientRect();
    place(hover, rect, false);
    if (!target || !hasArea(rect)) {
      hoverLabel.hidden = true;
      return;
    }
    const summary = elementSummary(target);
    const detail = summary.id ? `#${summary.id}` : summary.label || summary.text || "";
    hoverLabel.textContent = clipped(`<${summary.tag}> ${detail}`, 120);
    hoverLabel.hidden = false;
    hoverLabel.style.left = `${Math.max(4,
      Math.min(rect.left, innerWidth - hoverLabel.offsetWidth - 4))}px`;
    hoverLabel.style.top = `${Math.max(4, rect.top - 24)}px`;
  }, true);
  document.addEventListener("pointerdown", (event) => {
    const insideReviewUi = host && event.composedPath().includes(host);
    if (pending && !activeAnchor && !insideReviewUi) cancelPick(false);
    if (pickMode === "element" && event.button === 0 && !insideReviewUi) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }, true);
  document.addEventListener("click", (event) => {
    if (pickMode !== "element" || event.button !== 0 || event.composedPath().includes(host)) return;
    event.preventDefault(); event.stopImmediatePropagation();
    const element = elementFor(event.target);
    if (!element || !hasArea(element.getBoundingClientRect())) return;
    pickMode = "none";
    hover.hidden = true;
    hoverLabel.hidden = true;
    setPending(newSelection("element", null, element, safeText(element, 2000)), false);
    activeAnchor = { id: pending.selectionId, selection: pending.selection };
    post("anchor-picked", pending.message);
  }, true);
  document.addEventListener("keydown", (event) => event.key === "Escape" && cancelPick(true), true);
  const handleLayoutChange = scheduleOverlayUpdate;
  document.addEventListener("toggle", handleLayoutChange, true);
  document.addEventListener("load", (event) => {
    if (event.target instanceof HTMLImageElement) handleLayoutChange();
  }, true);
  document.addEventListener("transitionend", handleLayoutChange, true);
  document.addEventListener("animationend", handleLayoutChange, true);
  addEventListener("scroll", () => {
    scheduleOverlayUpdate();
  }, { passive: true, capture: true });
  addEventListener("scrollend", () => {
    scheduleOverlayUpdate();
    if (activeAnchor && activeAnchor.activateWhenVisible) activeAnchor = null;
  }, { passive: true, capture: true });
  addEventListener("resize", handleLayoutChange, { passive: true });
  const layoutObserver = new ResizeObserver(handleLayoutChange);
  layoutObserver.observe(document.documentElement);
  if (document.body) layoutObserver.observe(document.body);
  if (document.fonts) document.fonts.ready.then(handleLayoutChange);

  addEventListener("message", (event) => {
    const data = event.data;
    if (event.source !== parent || !data
        || data.scope !== scope || data.version !== version) return;
    if (data.type === "init" && data.documentId === documentId
        && typeof data.channelId === "string" && data.channelId) {
      if (channelId) {
        if (event.origin !== ownerOrigin || data.channelId !== channelId) return;
      } else {
        if (!allowedOwnerOrigins.has(event.origin)) return;
        ownerOrigin = event.origin;
        channelId = data.channelId;
      }
      ensureUi();
      const capabilities = markdownSource ? ["text", "region", "markers"]
        : ["text", "element", "region", "markers"];
      post("ready", { capabilities });
      return;
    }
    if (!channelId || event.origin !== ownerOrigin || data.channelId !== channelId) return;
    if (data.type === "pick-mode") {
      ensureUi();
      const requested = data.mode === "region" ? "region"
        : data.mode === "element" && !markdownSource ? "element" : "none";
      if (requested !== "none" && pending) cancelPick(false);
      pickMode = requested;
      hover.hidden = true;
      hoverLabel.hidden = true;
      regionPicker.setActive(requested === "region");
    } else if (data.type === "set-marker") addMarker(data.markerId, data.selectionId);
    else if (data.type === "remove-marker") removeMarker(data.markerId);
    else if (data.type === "focus-marker") focusMarker(data.markerId);
    else if (data.type === "cancel"
        && pending && pending.selectionId === data.anchorId) cancelPick(false);
    else if (data.type === "close-popover"
        && activeAnchor && activeAnchor.id === data.anchorId) activeAnchor = null;
  });
  const hello = () => {
    parent.postMessage({ scope, version, type: "hello", documentId }, "*");
  };
  hello();
  let attempts = 0;
  const helloTimer = setInterval(() => {
    if (channelId || ++attempts >= 12) clearInterval(helloTimer);
    else hello();
  }, 500);
})();
