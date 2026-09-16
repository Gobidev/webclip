(function () {
  "use strict";

  var MAX_SIZE = window.WEBCLIP_MAX_SIZE || 100000;
  var RECONNECT_DELAY = 1000;

  var field = document.getElementById("field");
  var box = document.getElementById("box");
  var textarea = document.getElementById("clipboard");
  var counter = document.getElementById("counter");
  var status = document.getElementById("status");

  textarea.maxLength = MAX_SIZE;

  function render() {
    counter.textContent = textarea.value.length + " / " + MAX_SIZE;
    box.classList.toggle("filled", textarea.value.length > 0);
  }

  function setConnected(connected) {
    field.classList.toggle("disabled", !connected);
    textarea.disabled = !connected;
    status.hidden = connected;
  }

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
    render();
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(textarea.value);
  });

  render();
  setConnected(false);
  connect();
})();
