function createPreviewRegionPicker({
  region,
  regionBox,
  regionStatus,
  host,
  renderer,
  getLayoutRevision,
  onPicked,
  onError,
  onCancel,
}) {
  let generation = 0;
  let start = null;

  const point = (event) => ({
    x: Math.max(0, Math.min(innerWidth, event.clientX)),
    y: Math.max(0, Math.min(innerHeight, event.clientY)),
  });
  const selectionRect = (from, to) => {
    const left = Math.min(from.x, to.x);
    const top = Math.min(from.y, to.y);
    return {
      x: left, y: top, left, top,
      right: Math.max(from.x, to.x), bottom: Math.max(from.y, to.y),
      width: Math.abs(to.x - from.x), height: Math.abs(to.y - from.y),
    };
  };
  const showRect = (rect) => {
    regionBox.hidden = false;
    Object.assign(regionBox.style, {
      left: `${rect.left}px`, top: `${rect.top}px`,
      width: `${rect.width}px`, height: `${rect.height}px`,
    });
  };
  const hide = () => {
    start = null;
    region.hidden = true;
    regionBox.hidden = true;
    regionStatus.hidden = true;
    region.style.cursor = "crosshair";
  };
  const cancel = () => {
    generation += 1;
    hide();
  };
  const setActive = (active) => {
    cancel();
    if (active) region.hidden = false;
  };
  const canvasBlob = (canvas) => new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => blob ? resolve(blob) : reject(new Error("Screenshot encoding failed.")),
      "image/png",
    );
  });
  const capture = async (rect) => {
    if (typeof renderer !== "function") {
      throw new Error("Screenshot renderer unavailable.");
    }
    const pixelBudget = 4_000_000;
    const naturalScale = Math.min(devicePixelRatio || 1, 2);
    const budgetScale = Math.sqrt(pixelBudget / Math.max(1, rect.width * rect.height));
    const canvas = await renderer(document.documentElement, {
      x: scrollX + rect.left,
      y: scrollY + rect.top,
      width: rect.width,
      height: rect.height,
      scrollX,
      scrollY,
      scale: Math.min(naturalScale, budgetScale),
      useCORS: true,
      imageTimeout: 5000,
      logging: false,
      ignoreElements: (element) => element === host,
    });
    return canvasBlob(canvas);
  };

  region.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    start = point(event);
    region.setPointerCapture(event.pointerId);
    showRect(selectionRect(start, start));
  });
  region.addEventListener("pointermove", (event) => {
    if (!start || !region.hasPointerCapture(event.pointerId)) return;
    showRect(selectionRect(start, point(event)));
  });
  region.addEventListener("pointercancel", () => {
    cancel();
    onCancel();
  });
  region.addEventListener("pointerup", async (event) => {
    if (!start || !region.hasPointerCapture(event.pointerId)) return;
    const rect = selectionRect(start, point(event));
    region.releasePointerCapture(event.pointerId);
    start = null;
    if (rect.width < 8 || rect.height < 8) {
      cancel();
      onCancel();
      return;
    }

    regionBox.hidden = true;
    region.hidden = true;
    const target = document.elementFromPoint(
      rect.left + rect.width / 2,
      rect.top + rect.height / 2,
    );
    const documentPoint = { x: rect.left + scrollX, y: rect.top + scrollY };
    const layoutRevision = getLayoutRevision();
    const currentGeneration = ++generation;
    region.hidden = false;
    region.style.cursor = "wait";
    regionStatus.hidden = false;
    try {
      const screenshot = await capture(rect);
      if (currentGeneration !== generation) return;
      if (layoutRevision !== getLayoutRevision()) {
        onError("Page changed while capturing. Try again.");
        return;
      }
      onPicked({ rect, target, screenshot, documentPoint });
    } catch (error) {
      if (currentGeneration === generation) {
        onError(error instanceof Error ? error.message : "Screenshot capture failed.");
      }
    } finally {
      if (currentGeneration === generation) hide();
    }
  });

  return { cancel, setActive };
}
