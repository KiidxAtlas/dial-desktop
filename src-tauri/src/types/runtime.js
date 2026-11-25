// The `window.dial` runtime every Type app gets, injected before the app's own scripts by
// `types::custom_page`. The page runs in a sandboxed frame with an opaque origin: it talks to
// Dial Desktop (`AppFrame.vue`) only by `postMessage`, and the host checks every request.
(() => {
  if (window.dial) return;
  const host = window.parent;
  const send = (message) => host.postMessage(Object.assign({ dial: 1 }, message), "*");
  let seq = 0;
  const waiting = new Map();
  const listeners = new Set();
  const call = (op, args) =>
    new Promise((resolve, reject) => {
      const id = ++seq;
      waiting.set(id, { resolve, reject });
      send({ kind: "call", id, op, args });
    });

  const show = (value) => {
    if (value instanceof Error) return value.stack || `${value.name}: ${value.message}`;
    if (typeof value === "string") return value;
    if (value instanceof Element) return value.outerHTML.slice(0, 300);
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  };
  const log = (level, args) => send({ kind: "log", level, text: Array.from(args, show).join(" ").slice(0, 2000) });
  for (const level of ["error", "warn", "info", "log"]) {
    const original = console[level];
    console[level] = function (...args) {
      log(level, args);
      return original.apply(this, args);
    };
  }
  addEventListener(
    "error",
    (event) => {
      const target = event.target;
      if (target && target !== window && (target.src || target.href)) {
        log("error", [`Failed to load ${target.tagName.toLowerCase()} ${target.src || target.href}`]);
      } else {
        log("error", [`${event.message}${event.lineno ? ` (line ${event.lineno}:${event.colno})` : ""}`, event.error ?? ""]);
      }
    },
    true,
  );
  addEventListener("unhandledrejection", (event) => log("error", ["Unhandled promise rejection:", event.reason]));

  const notify = () => {
    for (const listener of listeners) {
      try {
        listener(dial);
      } catch (error) {
        console.error(error);
      }
    }
  };

  const dial = {
    /** {id, name, description, icon, accent, fields} */
    type: null,
    items: [],
    state: {},
    images: {},
    ready: false,
    /** Call `listener(dial)` now (once data arrived) and whenever the type, items or state change. */
    onChange(listener) {
      listeners.add(listener);
      if (dial.ready) {
        try {
          listener(dial);
        } catch (error) {
          console.error(error);
        }
      }
      return () => listeners.delete(listener);
    },
    add: (fields) => call("add", { fields }),
    update: (itemId, changes = {}) => call("update", { itemId, fields: changes.fields, done: changes.done }),
    remove: (itemId) => call("remove", { itemId }),
    move: (itemId, to) => call("move", { itemId, to }),
    setState: (state) => {
      dial.state = state;
      return call("setState", { state });
    },
    /** A data: URL (or Blob/File) of a PNG, JPEG, GIF or WebP image → its asset name for an image field. */
    saveImage: async (image) => {
      const url = typeof image === "string" ? image : await new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(image);
      });
      const name = await call("saveImage", { data: String(url).replace(/^data:[^,]*,/, "") });
      return name;
    },
    /** A data: URL for an image field's value, or null while it loads. */
    image: (name) => (typeof name === "string" && dial.images[name]) || null,
    notify: (title, body = "") => call("notify", { title: String(title), body: String(body) }),
    open: (url) => call("open", { url: String(url) }),
  };
  window.dial = dial;

  // Links leave the sandbox through the host, in the user's browser.
  document.addEventListener("click", (event) => {
    const link = event.target instanceof Element && event.target.closest("a[href]");
    if (!link || event.defaultPrevented) return;
    const href = link.getAttribute("href") || "";
    if (/^(https?:|mailto:)/i.test(href)) {
      event.preventDefault();
      dial.open(link.href);
    }
  });

  const toJson = (value) => {
    if (value === undefined) return null;
    if (value instanceof Element) return value.outerHTML.slice(0, 4000);
    try {
      return JSON.parse(JSON.stringify(value));
    } catch {
      return String(value);
    }
  };

  addEventListener("message", (event) => {
    const message = event.data;
    if (event.source !== host || !message || message.dial !== 1) return;
    if (message.kind === "data") {
      dial.type = message.type;
      dial.items = message.items ?? [];
      dial.state = message.state ?? {};
      dial.images = message.images ?? {};
      dial.ready = true;
      notify();
    } else if (message.kind === "reply") {
      const pending = waiting.get(message.id);
      if (!pending) return;
      waiting.delete(message.id);
      if (message.ok) pending.resolve(message.value);
      else pending.reject(new Error(message.error));
    } else if (message.kind === "eval") {
      // The agent checking the app (`types_run`): a function body, awaited.
      Promise.resolve()
        .then(() => new Function("dial", `return (async () => {\n${message.script}\n})()`)(dial))
        .then(
          (value) => send({ kind: "eval-result", id: message.id, ok: true, value: toJson(value) }),
          (error) => send({ kind: "eval-result", id: message.id, ok: false, error: show(error) }),
        );
    }
  });

  send({ kind: "ready" });
  // After load, plus a moment for first paint. Not requestAnimationFrame: WebKit throttles it in
  // cross-origin frames the user has not interacted with yet.
  const loaded = () => setTimeout(() => send({ kind: "loaded" }), 120);
  if (document.readyState === "complete") loaded();
  else addEventListener("load", loaded);
})();
