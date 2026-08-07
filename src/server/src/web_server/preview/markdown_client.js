(function () {
  "use strict";

  const source = document.getElementById("markdown-source");
  const content = document.getElementById("content");
  const raw = JSON.parse(source.textContent);

  function escapeHtml(value) {
    const escapes = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return String(value).replace(/[&<>"']/g, (character) => escapes[character]);
  }

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
      return ["javascript:", "vbscript:", "data:"].includes(url.protocol) ? null : url.href;
    } catch {
      return null;
    }
  }

  const renderer = new marked.Renderer();
  renderer.html = ({ text }) => escapeHtml(text);
  renderer.image = ({ href, title, text }) => {
    const sourceUrl = absoluteHttpsUrl(href);
    if (!sourceUrl) return escapeHtml(text);
    const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";
    return `<img src="${escapeHtml(sourceUrl)}" alt="${escapeHtml(text)}"${titleAttribute}>`;
  };
  renderer.link = function ({ href, title, tokens }) {
    const label = this.parser.parseInline(tokens);
    const target = linkUrl(href);
    if (!target) return label;
    const titleAttribute = title ? ` title="${escapeHtml(title)}"` : "";
    return `<a href="${escapeHtml(target)}"${titleAttribute}>${label}</a>`;
  };

  content.innerHTML = marked.parse(raw, { gfm: true, renderer });
})();
