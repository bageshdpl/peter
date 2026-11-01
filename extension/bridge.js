// Runs on the Peter web app. Passes messages between the page and the
// extension, so the app can tell whether the extension is installed, connect
// it, and choose which session to record into.
(() => {
  const FROM_APP = "peter-app";
  const FROM_EXT = "peter-observer";

  function reply(message) {
    window.postMessage({ source: FROM_EXT, ...message }, window.location.origin);
  }

  function ask(message) {
    return new Promise((resolve) => {
      try {
        chrome.runtime.sendMessage(message, (response) => {
          resolve(chrome.runtime.lastError ? { ok: false, error: "The extension did not answer. Reload this page." } : response);
        });
      } catch {
        // The extension was reloaded or removed while this page was open.
        resolve({ ok: false, error: "The extension was updated. Reload this page." });
      }
    });
  }

  window.addEventListener("message", async (event) => {
    if (event.source !== window || event.origin !== window.location.origin) return;
    const msg = event.data;
    if (!msg || msg.source !== FROM_APP || typeof msg.type !== "string") return;

    if (msg.type === "ping") {
      const status = await ask({ type: "status" });
      reply({ type: "pong", version: chrome.runtime.getManifest().version, status });
    } else if (msg.type === "connect" && typeof msg.code === "string") {
      reply({ type: "connect-result", ...(await ask({ type: "connect", code: msg.code })) });
    } else if (msg.type === "select-session" && typeof msg.sessionId === "string") {
      reply({
        type: "select-result",
        ...(await ask({ type: "select-session", sessionId: msg.sessionId, title: String(msg.title ?? "") })),
      });
    }
  });

  // Let the page know as soon as the extension is present.
  reply({ type: "hello", version: chrome.runtime.getManifest().version });
})();
