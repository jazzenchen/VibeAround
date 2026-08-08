(function () {
  "use strict";

  var picker = document.getElementById("preview-picker");
  var frame = document.getElementById("preview-frame");
  var current = document.getElementById("current-preview");
  var refresh = document.getElementById("refresh-preview");

  picker.addEventListener("change", function () {
    var option = picker.options[picker.selectedIndex];
    var slug = encodeURIComponent(option.value);
    var title = option.dataset.title || option.textContent;
    frame.src = option.dataset.src;
    frame.title = "Preview content — " + title;
    current.textContent = title;
    document.title = "Preview — " + title;
    history.replaceState(null, "", "/va/preview/u/" + slug);
  });

  refresh.addEventListener("click", function () {
    location.reload();
  });
})();
