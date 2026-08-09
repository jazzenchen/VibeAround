(function () {
  "use strict";

  var picker = document.getElementById("preview-picker");
  var frame = document.getElementById("preview-frame");
  var current = document.getElementById("current-preview");
  var refresh = document.getElementById("refresh-preview");
  var chatRefresh = document.getElementById("preview-chat-refresh");

  function reloadPreview() {
    chatRefresh.hidden = true;
    frame.src = frame.src;
  }

  picker.addEventListener("change", function () {
    var option = picker.options[picker.selectedIndex];
    var slug = encodeURIComponent(option.value);
    var title = option.dataset.title || option.textContent;
    frame.src = option.dataset.src;
    frame.title = "Preview content — " + title;
    current.textContent = title;
    document.title = "Preview — " + title;
    chatRefresh.hidden = true;
    history.replaceState(null, "", "/va/preview/u/" + slug);
  });

  refresh.addEventListener("click", reloadPreview);
  chatRefresh.addEventListener("click", reloadPreview);
  document.addEventListener("va-preview-turn-complete", function () {
    chatRefresh.hidden = false;
  });
})();
