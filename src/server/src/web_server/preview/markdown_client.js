(function () {
  "use strict";

  const source = document.getElementById("markdown-source");
  const content = document.getElementById("content");
  const raw = JSON.parse(source.textContent);

  const allowedTags = [
    "a", "blockquote", "br", "code", "del", "details", "div", "em",
    "h1", "h2", "h3", "h4", "h5", "h6", "hr", "img", "input", "li",
    "ol", "p", "pre", "strong", "summary", "table", "tbody", "td", "th",
    "thead", "tr", "ul",
  ];
  const attributesByTag = {
    a: new Set(["href", "title"]),
    code: new Set(["class"]),
    details: new Set(["open"]),
    div: new Set(["align"]),
    img: new Set(["alt", "src", "title", "width"]),
    input: new Set(["checked", "disabled", "type"]),
    ol: new Set(["start"]),
    p: new Set(["align"]),
    table: new Set(["align"]),
    td: new Set(["align", "width"]),
    th: new Set(["align", "width"]),
  };
  const allowedAttributes = [...new Set(
    Object.values(attributesByTag).flatMap((attributes) => [...attributes]),
  )];

  function absoluteHttpsUrl(value) {
    try {
      const url = new URL(value);
      return /^https:\/\//i.test(value) && url.protocol === "https:" && url.hostname
        ? url.href
        : null;
    } catch {
      return null;
    }
  }

  function linkUrl(value) {
    try {
      const url = new URL(value, window.location.href);
      return ["http:", "https:", "mailto:"].includes(url.protocol) ? url.href : null;
    } catch {
      return null;
    }
  }

  function validAlign(value) {
    return ["left", "center", "right"].includes(value.toLowerCase());
  }

  function validWidth(value) {
    if (/^\d+$/.test(value)) {
      const pixels = Number(value);
      return pixels >= 1 && pixels <= 4096;
    }
    if (/^\d+%$/.test(value)) {
      const percent = Number(value.slice(0, -1));
      return percent >= 1 && percent <= 100;
    }
    return false;
  }

  function unwrap(element) {
    element.replaceWith(...Array.from(element.childNodes));
  }

  function tightenElement(element) {
    const tag = element.tagName.toLowerCase();
    const allowed = attributesByTag[tag] || new Set();
    for (const attribute of Array.from(element.attributes)) {
      if (!allowed.has(attribute.name)) element.removeAttribute(attribute.name);
    }

    if (element.hasAttribute("align") && !validAlign(element.getAttribute("align"))) {
      element.removeAttribute("align");
    }
    if (element.hasAttribute("width") && !validWidth(element.getAttribute("width"))) {
      element.removeAttribute("width");
    }
    if (tag === "code" && !/^language-[\w-]+$/.test(element.getAttribute("class") || "")) {
      element.removeAttribute("class");
    }
    if (tag === "ol" && !/^-?\d+$/.test(element.getAttribute("start") || "")) {
      element.removeAttribute("start");
    }
    if (tag === "details" && element.hasAttribute("open")) {
      element.setAttribute("open", "");
    }
    if (tag === "input") {
      const checked = element.hasAttribute("checked");
      element.setAttribute("type", "checkbox");
      element.setAttribute("disabled", "");
      element.toggleAttribute("checked", checked);
    }
    if (tag === "img") {
      const target = absoluteHttpsUrl(element.getAttribute("src") || "");
      if (!target) {
        element.replaceWith(document.createTextNode(element.getAttribute("alt") || ""));
        return;
      }
      element.setAttribute("src", target);
    }
    if (tag === "a") {
      const href = element.getAttribute("href");
      const target = href ? linkUrl(href) : null;
      if (!target) {
        unwrap(element);
        return;
      }
      element.setAttribute("href", target);
    }
  }

  const rendered = marked.parse(raw, { gfm: true });
  const fragment = DOMPurify.sanitize(rendered, {
    ALLOWED_ATTR: allowedAttributes,
    ALLOWED_TAGS: allowedTags,
    ALLOW_ARIA_ATTR: false,
    ALLOW_DATA_ATTR: false,
    RETURN_DOM_FRAGMENT: true,
  });
  for (const element of Array.from(fragment.querySelectorAll("*"))) {
    tightenElement(element);
  }
  content.replaceChildren(fragment);
})();
