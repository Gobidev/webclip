(function () {
  "use strict";

  var MAX_SIZE = window.WEBCLIP_MAX_SIZE || 100000;
  var RECONNECT_DELAY = 1000;
  var PRIVATE_ENABLED = window.WEBCLIP_PRIVATE_ENABLED !== false;
  var PRIVATE_PIN_LENGTH = window.WEBCLIP_PRIVATE_PIN_LENGTH || 4;

  var tabShared = document.getElementById("tab-shared");
  var tabPrivate = document.getElementById("tab-private");
  var panelShared = document.getElementById("panel-shared");
  var panelPrivate = document.getElementById("panel-private");

  function copyText(text, button, doneLabel) {
    if (!text) return;

    function done() {
      var original = button.textContent;
      button.textContent = doneLabel;
      clearTimeout(button.copyTimer);
      button.copyTimer = setTimeout(function () {
        button.textContent = original;
      }, 1500);
    }

    if (navigator.clipboard && window.isSecureContext) {
      navigator.clipboard.writeText(text).then(done, function () {});
    } else {
      var helper = document.createElement("textarea");
      helper.value = text;
      helper.setAttribute("readonly", "");
      helper.style.position = "fixed";
      helper.style.top = "-1000px";
      document.body.appendChild(helper);
      helper.select();
      try {
        document.execCommand("copy");
        done();
      } catch (error) {
        /* copying is not available */
      }
      document.body.removeChild(helper);
    }
  }

  function showStatus(element, message, isError) {
    element.textContent = message || "";
    element.hidden = !message;
    element.classList.toggle("error", !!isError);
  }

  function selectTab(isPrivate) {
    tabShared.classList.toggle("active", !isPrivate);
    tabPrivate.classList.toggle("active", isPrivate);
    tabShared.setAttribute("aria-selected", String(!isPrivate));
    tabPrivate.setAttribute("aria-selected", String(isPrivate));
    panelShared.hidden = isPrivate;
    panelPrivate.hidden = !isPrivate;
  }

  tabShared.addEventListener("click", function () {
    selectTab(false);
  });

  tabPrivate.addEventListener("click", function () {
    selectTab(true);
  });

  // Opens a clipboard websocket and reconnects after connection loss. A close
  // with code 1008 means the private PIN was rejected and is never retried.
  function openSocket(options) {
    var ws = null;
    var closed = false;
    var reconnectTimer = null;

    function start() {
      var scheme = location.protocol === "https:" ? "wss" : "ws";
      ws = new WebSocket(scheme + "://" + location.host + options.url);

      ws.addEventListener("open", function () {
        if (options.auth) ws.send(options.auth);
        if (options.onOpen) options.onOpen();
      });

      ws.addEventListener("message", function (event) {
        if (typeof event.data !== "string") return;
        if (options.onMessage) options.onMessage(event.data);
      });

      ws.addEventListener("close", function (event) {
        if (options.onClose) options.onClose(event);
        if (!closed && event.code !== 1008) {
          clearTimeout(reconnectTimer);
          reconnectTimer = setTimeout(start, RECONNECT_DELAY);
        }
      });

      ws.addEventListener("error", function () {
        ws.close();
      });
    }

    start();

    return {
      send: function (text) {
        if (ws && ws.readyState === WebSocket.OPEN) ws.send(text);
      },
      close: function () {
        closed = true;
        clearTimeout(reconnectTimer);
        if (ws) ws.close();
      },
    };
  }

  // Shared clipboard

  var field = document.getElementById("field");
  var box = document.getElementById("box");
  var textarea = document.getElementById("clipboard");
  var counter = document.getElementById("counter");
  var status = document.getElementById("status");
  var copyButton = document.getElementById("copy");
  var clearButton = document.getElementById("clear");

  var sharedSocket = null;
  var connected = false;

  textarea.maxLength = MAX_SIZE;

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

  copyButton.addEventListener("click", function () {
    copyText(textarea.value, copyButton, "Copied");
  });

  clearButton.addEventListener("click", function () {
    if (!textarea.value) return;
    textarea.value = "";
    render();
    if (sharedSocket) sharedSocket.send("");
  });

  textarea.addEventListener("input", function () {
    render();
    if (sharedSocket) sharedSocket.send(textarea.value);
  });

  // Private clipboard

  function initPrivate() {
    var start = document.getElementById("private-start");
    var startStatus = document.getElementById("private-start-status");
    var pinInput = document.getElementById("private-pin-input");
    var joinButton = document.getElementById("private-join");
    var createButton = document.getElementById("private-create");

    var room = document.getElementById("private-room");
    var roomBox = document.getElementById("private-room-box");
    var roomText = document.getElementById("private-room-text");
    var roomCopy = document.getElementById("private-room-copy");
    var roomClear = document.getElementById("private-room-clear");
    var roomLeave = document.getElementById("private-room-leave");
    var roomPin = document.getElementById("private-room-pin");
    var roomStatus = document.getElementById("private-room-status");

    var socket = null;
    var currentPin = "";
    var joined = false;
    var connected = false;
    var creating = false;

    pinInput.maxLength = PRIVATE_PIN_LENGTH;
    roomText.maxLength = MAX_SIZE;

    function renderStart() {
      joinButton.disabled = pinInput.value.length !== PRIVATE_PIN_LENGTH;
      createButton.disabled = creating;
    }

    function renderRoom() {
      var empty = roomText.value.length === 0;
      roomBox.classList.toggle("filled", !empty);
      roomCopy.disabled = !connected || empty;
      roomClear.disabled = !connected || empty;
    }

    function setRoomConnected(value) {
      connected = value;
      room.classList.toggle("disabled", !value);
      roomText.disabled = !value;
      renderRoom();
    }

    function showStart() {
      room.hidden = true;
      start.hidden = false;
    }

    function showRoom() {
      start.hidden = true;
      room.hidden = false;
      roomPin.textContent = "PIN " + currentPin;
    }

    function leave(clearPin) {
      if (socket) socket.close();
      socket = null;
      joined = false;
      setRoomConnected(false);
      roomText.value = "";
      if (clearPin !== false) pinInput.value = "";
      showStatus(roomStatus, "");
      showStatus(startStatus, "");
      showStart();
      renderStart();
      pinInput.focus();
    }

    function join(pin) {
      currentPin = pin;
      joined = false;
      setRoomConnected(false);
      showStatus(roomStatus, "Connecting", false);
      showRoom();
      socket = openSocket({
        url: "/ws/private",
        auth: pin,
        onMessage: function (text) {
          if (!joined) {
            joined = true;
            showStatus(roomStatus, "");
          }
          setRoomConnected(true);
          roomText.value = text;
          renderRoom();
        },
        onClose: function (event) {
          setRoomConnected(false);
          if (event.code === 1008) {
            var message =
              event.reason.indexOf("rate") === 0
                ? "Too many attempts, try again in a minute"
                : "Invalid or expired PIN";
            leave(false);
            showStatus(startStatus, message, true);
            return;
          }
          if (joined) {
            showStatus(roomStatus, "Reconnecting", false);
          } else {
            showStatus(roomStatus, "Connecting", false);
          }
        },
      });
    }

    function joinFromInput() {
      var pin = pinInput.value.replace(/\D/g, "").slice(0, PRIVATE_PIN_LENGTH);
      pinInput.value = pin;
      if (pin.length !== PRIVATE_PIN_LENGTH) {
        showStatus(startStatus, "Enter the " + PRIVATE_PIN_LENGTH + "-digit PIN", true);
        return;
      }
      join(pin);
    }

    function create() {
      if (creating) return;
      creating = true;
      renderStart();
      showStatus(startStatus, "");
      fetch("/private", { method: "POST" })
        .then(function (response) {
          if (!response.ok) throw new Error("create failed");
          return response.text().then(function (pin) {
            pinInput.value = pin.trim();
            join(pinInput.value);
          });
        })
        .catch(function () {
          showStatus(startStatus, "Could not create a private clipboard", true);
        })
        .then(function () {
          creating = false;
          renderStart();
        });
    }

    pinInput.addEventListener("input", function () {
      var digits = pinInput.value.replace(/\D/g, "").slice(0, PRIVATE_PIN_LENGTH);
      if (pinInput.value !== digits) pinInput.value = digits;
      showStatus(startStatus, "");
      renderStart();
    });

    pinInput.addEventListener("keydown", function (event) {
      if (event.key === "Enter" && !joinButton.disabled) {
        event.preventDefault();
        joinFromInput();
      }
    });

    joinButton.addEventListener("click", joinFromInput);
    createButton.addEventListener("click", create);

    roomText.addEventListener("input", function () {
      renderRoom();
      if (socket) socket.send(roomText.value);
    });

    roomCopy.addEventListener("click", function () {
      copyText(roomText.value, roomCopy, "Copied");
    });

    roomClear.addEventListener("click", function () {
      if (!roomText.value) return;
      roomText.value = "";
      renderRoom();
      if (socket) socket.send("");
    });

    roomLeave.addEventListener("click", function () {
      leave(true);
    });

    roomPin.addEventListener("click", function () {
      copyText(currentPin, roomPin, "Copied");
    });

    renderStart();
    renderRoom();
  }

  if (PRIVATE_ENABLED) {
    initPrivate();
  } else {
    tabPrivate.hidden = true;
  }

  sharedSocket = openSocket({
    url: "/ws",
    onOpen: function () {
      setConnected(true);
    },
    onMessage: function (text) {
      textarea.value = text;
      render();
    },
    onClose: function () {
      setConnected(false);
    },
  });

  render();
  setConnected(false);
})();
