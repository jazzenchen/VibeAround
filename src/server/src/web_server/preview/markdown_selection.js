(function () {
  "use strict";

  var content = document.getElementById("content");
  var pathMatch = location.pathname.match(/\/preview\/u\/([^/]+)\/content$/);
  if (!content || window.parent === window || !pathMatch) return;

  var previewSlug;
  try { previewSlug = decodeURIComponent(pathMatch[1]); } catch (_) { return; }

  var maxSelectionLength = 4000;
  var pending = null;
  var trigger = document.createElement("button");
  trigger.type = "button";
  trigger.className = "preview-comment-trigger";
  trigger.textContent = "Add comment";
  trigger.hidden = true;
  document.body.appendChild(trigger);

  function elementFor(node) {
    return node && (node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement);
  }

  function insideContent(node) {
    var element = elementFor(node);
    return element === content || (element && content.contains(element));
  }

  function nearestHeading(range) {
    var start = elementFor(range.startContainer);
    var result = "";
    for (var heading of content.querySelectorAll("h1, h2, h3, h4, h5, h6")) {
      if (heading === start || heading.contains(start)
          || (heading.compareDocumentPosition(start) & Node.DOCUMENT_POSITION_FOLLOWING)) {
        result = (heading.textContent || "").trim();
      } else {
        break;
      }
    }
    return result.slice(0, 240);
  }

  function hideTrigger() {
    trigger.hidden = true;
    pending = null;
  }

  function updateTrigger() {
    var selection = window.getSelection();
    if (!selection || selection.isCollapsed || selection.rangeCount === 0
        || !insideContent(selection.anchorNode) || !insideContent(selection.focusNode)) {
      hideTrigger();
      return;
    }

    var range = selection.getRangeAt(0);
    var fullText = range.toString().trim();
    if (!fullText) {
      hideTrigger();
      return;
    }
    var rects = Array.from(range.getClientRects()).filter(function (rect) {
      return rect.width > 0 || rect.height > 0;
    });
    var rect = rects[rects.length - 1] || range.getBoundingClientRect();
    pending = {
      text: fullText.slice(0, maxSelectionLength),
      truncated: fullText.length > maxSelectionLength,
      heading: nearestHeading(range),
    };
    trigger.hidden = false;
    trigger.style.left = Math.max(8, Math.min(rect.left, window.innerWidth - 112)) + "px";
    trigger.style.top = Math.max(8, Math.min(rect.bottom + 8, window.innerHeight - 40)) + "px";
  }

  trigger.addEventListener("pointerdown", function (event) { event.preventDefault(); });
  trigger.addEventListener("click", function () {
    if (!pending) return;
    window.parent.postMessage({
      type: "va.preview.markdown-selection",
      version: 1,
      previewSlug: previewSlug,
      selection: pending,
    }, window.location.origin);
    window.getSelection().removeAllRanges();
    hideTrigger();
  });
  document.addEventListener("selectionchange", updateTrigger);
  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape") hideTrigger();
  });
  window.addEventListener("scroll", hideTrigger, { passive: true });
  window.addEventListener("resize", hideTrigger);
})();
