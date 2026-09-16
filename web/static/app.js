(function () {
  "use strict";

  var MAX_SIZE = window.WEBCLIP_MAX_SIZE || 100000;
  var RECONNECT_DELAY = 1000;

  var field = document.getElementById("field");
  var box = document.getElementById("box");
  var textarea = document.getElementById("clipboard");
  var counter = document.getElementById("counter");
  var status = document.getElementById("status");
  var copyButton = document.getElementById("copy");
  var clearButton = document.getElementById("clear");

  textarea.maxLength = MAX_SIZE;

  var connected = false;
  var copyTimer = null;

  function render() {
    var empty = textarea.value.length === 0;
    counter.textContent = textarea.value.length + " / " + MAX_SIZE;
    box.classList.toggle("filled", !empty);
    copyButton.disabled = !connected || empty;
    clearButton.disabled = !connected || empty;
  }

  function setConnected(value) {
    connected = value;
    field.classList.toggle("disabled", !connected);
    textarea.disabled = !connected;
    status.hidden = connected;
    render();
  }

  function resetCopyButton() {
    clearTimeout(copyTimer);
    copyTimer = null;
    copyButton.textContent = "Copy";
  }

  function copyAll() {
    var text = textarea.value;
    if (!text) return;

    function done() {
      copyButton.textContent = "Copied";
      clearTimeout(copyTimer);
      copyTimer = setTimeout(resetCopyButton, 1500);
    }

    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(text).then(done, function () {});
    } else {
      textarea.focus();
      textarea.select();
      try {
        document.execCommand("copy");
        done();
      } catch (error) {
        /* copying is not available */
      }
      textarea.setSelectionRange(textarea.value.length, textarea.value.length);
      textarea.blur();
    }
  }

  function clearAll() {
    if (!textarea.value) return;
    textarea.value = "";
    render();
    if (ws && ws.readyState === WebSocket.OPEN) ws.send("");
  }

  copyButton.addEventListener("click", copyAll);
  clearButton.addEventListener("click", clearAll);

  var ws = null;

  function connect() {
    var scheme = location.protocol === "https:" ? "wss" : "ws";
    ws = new WebSocket(scheme + "://" + location.host + "/ws");

    ws.addEventListener("open", function () {
      setConnected(true);
    });

    ws.addEventListener("message", function (event) {
      if (typeof event.data !== "string") return;
      textarea.value = event.data;
      render();
    });

    ws.addEventListener("close", function () {
      setConnected(false);
      setTimeout(connect, RECONNECT_DELAY);
    });

    ws.addEventListener("error", function () {
      ws.close();
    });
  }

  textarea.addEventListener("input", function () {
    resetCopyButton();
    render();
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(textarea.value);
  });

  render();
  setConnected(false);
  connect();
})();
