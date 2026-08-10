(function () {
  "use strict";

  var picker = document.getElementById("preview-picker");
  var frame = document.getElementById("preview-frame");
  var toggle = document.getElementById("preview-chat-toggle");
  var form = document.getElementById("preview-chat-form");
  var input = document.getElementById("preview-chat-input");
  var chips = document.getElementById("preview-review-chips");
  var count = document.getElementById("preview-review-count");
  var feedback = document.getElementById("preview-review-feedback");
  var selectElement = document.getElementById("preview-select-element");
  var modeNotice = document.getElementById("preview-review-mode");
  var popover = document.getElementById("preview-comment-popover");
  var locationNode = document.getElementById("preview-comment-location");
  var selectionNode = document.getElementById("preview-comment-selection");
  var commentInput = document.getElementById("preview-comment-input");
  var cancel = document.getElementById("preview-comment-cancel");
  var items = [];
  var editing = null;
  var elementMode = false;
  var reviewPrompt = window.VAPreviewReviewPrompt;

  function selectedOption() {
    return picker.options[picker.selectedIndex];
  }

  function makeId() {
    return typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : "review-" + Date.now() + "-" + Math.random().toString(16).slice(2);
  }

  function excerpt(text, limit) {
    var compact = String(text || "").replace(/\s+/g, " ").trim();
    return compact.length > limit ? compact.slice(0, limit - 1) + "…" : compact;
  }

  function setFeedback(text) {
    feedback.textContent = text;
    feedback.hidden = !text;
  }

  function setElementMode(enabled) {
    elementMode = Boolean(enabled);
    selectElement.setAttribute("aria-pressed", elementMode ? "true" : "false");
    modeNotice.hidden = !elementMode;
    if (window.VAPreviewFrameReview) {
      window.VAPreviewFrameReview.setElementMode(elementMode);
    }
  }

  function hidePopover(cancelPick) {
    var previous = editing;
    popover.hidden = true;
    if (previous && window.VAPreviewFrameReview) {
      if (cancelPick && !previous.id) window.VAPreviewFrameReview.cancelPick();
      else window.VAPreviewFrameReview.closePopover();
    }
    editing = null;
  }

  function positionPopover(rect) {
    if (!rect || rect.y + rect.height <= 0 || rect.y >= frame.clientHeight
        || rect.x + rect.width <= 0 || rect.x >= frame.clientWidth) {
      hidePopover(true);
      return;
    }
    if (editing) editing.rect = rect;
    var frameRect = frame.getBoundingClientRect();
    var width = Math.min(360, window.innerWidth - 24);
    popover.style.width = width + "px";
    popover.hidden = false;
    var height = popover.offsetHeight;
    var left = frameRect.left + rect.x;
    var top = frameRect.top + rect.y + rect.height + 8;
    left = Math.max(12, Math.min(left, window.innerWidth - width - 12));
    if (top + height > window.innerHeight - 12) {
      top = frameRect.top + rect.y - height - 8;
    }
    popover.style.left = left + "px";
    popover.style.top = Math.max(12, top) + "px";
  }

  function openPopover(anchor, rect, item, selectionId) {
    editing = {
      id: item ? item.id : null,
      anchor: anchor,
      selectionId: selectionId || null,
      anchorId: item ? item.id : selectionId,
      rect: rect,
    };
    locationNode.textContent = reviewPrompt.locationText(anchor);
    selectionNode.textContent = excerpt(reviewPrompt.quote(anchor), 320);
    commentInput.value = item ? item.comment : "";
    setFeedback("");
    positionPopover(rect);
    commentInput.focus();
  }

  function createChip(item) {
    var chip = document.createElement("div");
    var focus = document.createElement("button");
    var icon = document.createElement("span");
    var label = document.createElement("span");
    var remove = document.createElement("button");
    chip.className = "review-chip";
    focus.type = "button";
    focus.className = "review-chip-focus";
    focus.title = reviewPrompt.locationText(item.anchor);
    icon.className = "review-chip-icon";
    icon.setAttribute("aria-hidden", "true");
    label.textContent = excerpt(reviewPrompt.quote(item.anchor), 54);
    focus.append(icon, label);
    focus.addEventListener("click", function () {
      if (window.VAPreviewFrameReview) window.VAPreviewFrameReview.focusMarker(item.id);
    });
    remove.type = "button";
    remove.className = "review-chip-remove";
    remove.setAttribute("aria-label", "Remove comment on " + excerpt(reviewPrompt.quote(item.anchor), 40));
    remove.textContent = "×";
    remove.addEventListener("click", function () {
      items = items.filter(function (candidate) { return candidate.id !== item.id; });
      if (editing && editing.id === item.id) hidePopover(false);
      if (window.VAPreviewFrameReview) window.VAPreviewFrameReview.removeMarker(item.id);
      render();
    });
    chip.append(focus, remove);
    return chip;
  }

  function render() {
    chips.replaceChildren(...items.map(createChip));
    chips.hidden = items.length === 0;
    count.textContent = String(items.length);
    count.hidden = items.length === 0;
    toggle.setAttribute("aria-label", items.length
      ? "Preview conversation, " + items.length + (items.length === 1 ? " draft review note" : " draft review notes")
      : "Preview conversation");
  }

  popover.addEventListener("submit", function (event) {
    event.preventDefault();
    if (!editing) return;
    var comment = commentInput.value.trim();
    if (!comment) {
      commentInput.focus();
      return;
    }
    var item = editing.id && items.find(function (candidate) { return candidate.id === editing.id; });
    if (item) item.comment = comment;
    else {
      item = { id: makeId(), anchor: editing.anchor, comment: comment };
      items.push(item);
    }
    if (window.VAPreviewFrameReview) {
      window.VAPreviewFrameReview.setMarker(item.id, item.anchor, editing.selectionId);
    }
    hidePopover(false);
    render();
  });

  cancel.addEventListener("click", function () { hidePopover(true); });
  commentInput.addEventListener("keydown", function (event) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      popover.requestSubmit();
    }
  });
  selectElement.addEventListener("click", function () {
    setElementMode(!elementMode);
    if (elementMode && toggle.getAttribute("aria-expanded") === "true") toggle.click();
  });
  form.addEventListener("submit", function (event) {
    if (!items.length) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    var prompt = input.value.trim();
    var message = reviewPrompt.build(selectedOption(), items, prompt);
    var chat = window.VAPreviewChat;
    if (!chat || !chat.canSend()) {
      setFeedback("Wait for the conversation to connect and finish its current turn.");
      return;
    }
    if (message.length > input.maxLength) {
      setFeedback("These review notes are too long for one message.");
      return;
    }
    var summary = reviewPrompt.display(items, prompt);
    if (!chat.send(message, summary)) return;
    items.forEach(function (item) {
      if (window.VAPreviewFrameReview) window.VAPreviewFrameReview.removeMarker(item.id);
    });
    items = [];
    input.value = "";
    input.dispatchEvent(new Event("input"));
    setFeedback("");
    render();
  }, true);
  document.addEventListener("keydown", function (event) {
    if (event.key !== "Escape" || (popover.hidden && !elementMode)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!popover.hidden) hidePopover(true);
    if (elementMode) setElementMode(false);
  }, true);
  document.addEventListener("pointerdown", function (event) {
    if (!popover.hidden && !popover.contains(event.target)) hidePopover(true);
  }, true);
  window.addEventListener("blur", function () {
    setTimeout(function () {
      if (!popover.hidden && document.activeElement === frame) hidePopover(true);
    }, 0);
  });
  window.addEventListener("resize", function () {
    if (editing && editing.rect) positionPopover(editing.rect);
  });

  window.VAPreviewComments = {
    setCapabilities: function (capabilities) {
      var supportsElements = Array.isArray(capabilities)
        ? capabilities.includes("element")
        : Boolean(capabilities && capabilities.element);
      selectElement.hidden = !supportsElements;
      if (!supportsElements && elementMode) setElementMode(false);
    },
    pick: function (anchor, rect, selectionId) {
      if (!anchor || !rect) return;
      if (elementMode) setElementMode(false);
      openPopover(anchor, rect, null, selectionId);
    },
    activate: function (id, rect) {
      var item = items.find(function (candidate) { return candidate.id === id; });
      if (item && rect) openPopover(item.anchor, rect, item, null);
    },
    reposition: function (anchorId, rect) {
      if (!editing || editing.anchorId !== anchorId) return;
      positionPopover(rect);
    },
    cancelPick: function () {
      elementMode = false;
      selectElement.setAttribute("aria-pressed", "false");
      modeNotice.hidden = true;
    },
    resetFrame: function () {
      items = [];
      hidePopover(false);
      setFeedback("");
      setElementMode(false);
      render();
    },
  };

  render();
})();
