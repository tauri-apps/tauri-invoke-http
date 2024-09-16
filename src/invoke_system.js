function processIpcMessage(message) {
  if (
    message instanceof ArrayBuffer ||
    ArrayBuffer.isView(message) ||
    Array.isArray(message)
  ) {
    return {
      contentType: "application/octet-stream",
      data: message,
    };
  } else {
    const data = JSON.stringify(message, (_k, val) => {
      if (val instanceof Map) {
        let o = {};
        val.forEach((v, k) => (o[k] = v));
        return o;
      } else if (val instanceof Uint8Array) {
        return Array.from(val);
      } else if (val instanceof ArrayBuffer) {
        return Array.from(new Uint8Array(val));
      } else if (
        val instanceof Object &&
        "__TAURI_CHANNEL_MARKER__" in val &&
        typeof val.id === "number"
      ) {
        return `__CHANNEL__:${val.id}`;
      } else {
        return val;
      }
    });

    return {
      contentType: "application/json",
      data,
    };
  }
}

const port = __PORT__;

(function () {
  function sendIpcMessage(message) {
    const { cmd, callback, error, payload, options } = message;

    const { contentType, data } = processIpcMessage(payload);
    const windowLabel = window.__TAURI_INTERNALS__.metadata.currentWindow.label;
    fetch(`http://localhost:${port}/${windowLabel}/${cmd}`, {
      method: "POST",
      body: data,
      headers: {
        "Content-Type": contentType,
        "Tauri-Callback": callback,
        "Tauri-Error": error,
        "Tauri-Invoke-Key": __INVOKE_KEY__,
        ...((options && options.headers) || {}),
      },
    })
      .then((response) => {
        console.log("res", response.headers.get("Tauri-Response"));
        const cb =
          response.headers.get("Tauri-Response") === "ok" ? callback : error;
        // we need to split here because on Android the content-type gets duplicated
        switch ((response.headers.get("content-type") || "").split(",")[0]) {
          case "application/json":
            return response.json().then((r) => [cb, r]);
          case "text/plain":
            return response.text().then((r) => [cb, r]);
          default:
            return response.arrayBuffer().then((r) => [cb, r]);
        }
      })
      .then(([cb, data]) => {
        if (window[`_${cb}`]) {
          window[`_${cb}`](data);
        } else {
          console.warn(
            `[TAURI] Couldn't find callback id {cb} in window. This might happen when the app is reloaded while Rust is running an asynchronous operation.`
          );
        }
      });
  }

  Object.defineProperty(window.__TAURI_INTERNALS__, "postMessage", {
    value: sendIpcMessage,
  });
})();
