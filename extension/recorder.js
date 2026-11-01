// Injected into the tab being recorded. Turns what the tester does into
// steps: clicks, typed values, choices and form submissions. Runs only on
// sites the tester allowed, and only while recording.
(() => {
  if (window.__peterRecorder) {
    window.__peterRecorder.resume();
    return;
  }

  let active = true;
  const MASK = "•••• (hidden)";
  const SENSITIVE_NAME = /pass|pwd|secret|token|otp|one.?time|pin\b|ssn|social|card|cvv|cvc|iban|account.?num/i;
  const SENSITIVE_AUTOCOMPLETE = /current-password|new-password|one-time-code|cc-/;

  /** Whether a field's value must never leave the browser. */
  function isSensitive(el) {
    if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) return false;
    if (el.type === "password" || el.type === "hidden") return true;
    if (el.closest("[data-peter-mask]")) return true;
    const autocomplete = el.getAttribute("autocomplete") || "";
    const names = [el.name, el.id, el.getAttribute("aria-label") || "", el.placeholder || ""].join(" ");
    return SENSITIVE_AUTOCOMPLETE.test(autocomplete) || SENSITIVE_NAME.test(names);
  }

  /** Hides sensitive values on screen too, so screenshots cannot show them. */
  function maskOnScreen() {
    for (const el of document.querySelectorAll("input, textarea")) {
      if (isSensitive(el) && el.type !== "password") el.style.setProperty("-webkit-text-security", "disc");
    }
  }

  const clean = (text, max = 120) => (text || "").replace(/\s+/g, " ").trim().slice(0, max);

  function textOf(id) {
    const el = id && document.getElementById(id);
    return el ? clean(el.textContent) : "";
  }

  /** A <label>'s own words, without the text of controls inside it (such as a select's options). */
  function labelText(label) {
    const copy = label.cloneNode(true);
    copy.querySelectorAll("input, select, textarea, button, option").forEach((n) => n.remove());
    return clean(copy.textContent);
  }

  /** The name a person would use for an element. */
  function labelOf(el) {
    const aria = el.getAttribute("aria-label");
    if (aria) return clean(aria);
    const labelledBy = el.getAttribute("aria-labelledby");
    if (labelledBy) {
      const text = labelledBy.split(/\s+/).map(textOf).join(" ");
      if (text.trim()) return clean(text);
    }
    if (el.labels && el.labels.length) {
      const text = labelText(el.labels[0]);
      if (text) return text;
    }
    if (el instanceof HTMLInputElement && ["submit", "button", "reset"].includes(el.type)) return clean(el.value);
    const own = clean(el.innerText || el.textContent);
    if (own) return own;
    return clean(el.getAttribute("title") || el.getAttribute("alt") || el.getAttribute("placeholder") || el.getAttribute("name") || el.tagName.toLowerCase());
  }

  /** A short CSS path, good enough to find the element again by eye. */
  function selectorOf(el) {
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && parts.length < 4) {
      if (node.id && /^[A-Za-z][\w-]*$/.test(node.id)) {
        parts.unshift(`#${node.id}`);
        break;
      }
      let part = node.tagName.toLowerCase();
      const parent = node.parentElement;
      if (parent) {
        const same = Array.from(parent.children).filter((c) => c.tagName === node.tagName);
        if (same.length > 1) part += `:nth-of-type(${same.indexOf(node) + 1})`;
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(" > ").slice(0, 500);
  }

  /**
   * Briefly draws a box around the element so the screenshot shows what was
   * used. The box is a separate overlay, so the page's own styles are never
   * changed (and overlapping steps cannot leave outlines behind).
   */
  // One box, moved to each new element, so a screenshot shows only the latest.
  let box = null;
  let hideTimer = 0;
  function flash(el) {
    const rect = el.getBoundingClientRect();
    if (!rect.width && !rect.height) return;
    box ??= document.createElement("div");
    box.setAttribute("aria-hidden", "true");
    Object.assign(box.style, {
      position: "fixed",
      left: `${rect.left - 4}px`,
      top: `${rect.top - 4}px`,
      width: `${rect.width + 8}px`,
      height: `${rect.height + 8}px`,
      border: "3px solid #e8590c",
      borderRadius: "4px",
      pointerEvents: "none",
      zIndex: "2147483647",
      boxSizing: "border-box",
    });
    if (!box.isConnected) (document.body || document.documentElement).appendChild(box);
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => box.remove(), 700);
  }

  function send(step, el) {
    if (!active) return;
    maskOnScreen();
    if (el) flash(el);
    const message = {
      type: "step",
      step: {
        url: location.href,
        page_title: clean(document.title, 300),
        captured_at_ms: Date.now(),
        target_selector: el ? selectorOf(el) : "",
        ...step,
      },
    };
    // Screenshots show the last frame the browser drew, so wait until the
    // change and the highlight box are painted. If the page is leaving (a link
    // was followed), send at once so the step is not lost.
    let sent = false;
    const fire = (event) => {
      if (sent) return;
      sent = true;
      window.removeEventListener("pagehide", fire);
      // The page is already going away, so a screenshot now would show the
      // next page, not this step. The next page gets its own step.
      if (event?.type === "pagehide") message.step.skip_screenshot = true;
      try {
        chrome.runtime.sendMessage(message);
      } catch {
        // The extension was reloaded; this copy of the recorder is orphaned.
        active = false;
      }
    };
    window.addEventListener("pagehide", fire);
    requestAnimationFrame(() => requestAnimationFrame(() => fire()));
    setTimeout(() => fire(), 200); // Hidden tabs do not paint.
  }

  const CLICKABLE =
    "button, a[href], [role=button], [role=link], [role=menuitem], [role=tab], [role=checkbox], [role=radio], [role=option], " +
    "input[type=submit], input[type=button], input[type=reset], input[type=checkbox], input[type=radio], summary, label, [onclick]";

  // A click on a submit button already records the submission.
  let lastSubmitClick = { form: null, at: 0 };

  document.addEventListener(
    "click",
    (event) => {
      const raw = event.target instanceof Element ? event.target : null;
      if (!raw) return;
      const submitter = raw.closest("button:not([type=button]):not([type=reset]), input[type=submit]");
      if (submitter?.form) lastSubmitClick = { form: submitter.form, at: Date.now() };
      // Typing into a field is recorded by the change, not the click.
      if (raw.matches("input:not([type=submit]):not([type=button]):not([type=reset]):not([type=checkbox]):not([type=radio]), textarea, select")) return;
      const el = raw.closest(CLICKABLE) || raw;
      let label = labelOf(el);
      if (el instanceof HTMLInputElement && (el.type === "checkbox" || el.type === "radio")) {
        label = `${label} (${el.checked ? "checked" : "unchecked"})`;
      }
      send({ kind: "click", target_label: label }, el);
    },
    true,
  );

  document.addEventListener(
    "change",
    (event) => {
      const el = event.target;
      if (el instanceof HTMLSelectElement) {
        const chosen = Array.from(el.selectedOptions).map((o) => clean(o.textContent)).join(", ");
        send({ kind: "select", target_label: labelOf(el), value: chosen }, el);
      } else if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
        if (el instanceof HTMLInputElement && ["checkbox", "radio", "submit", "button", "file"].includes(el.type)) {
          if (el.type === "file") send({ kind: "input", target_label: labelOf(el), value: `${el.files?.length ?? 0} file(s) chosen` }, el);
          return;
        }
        const sensitive = isSensitive(el);
        send({ kind: "input", target_label: labelOf(el), value: sensitive ? MASK : el.value.slice(0, 2000), masked: sensitive }, el);
      }
    },
    true,
  );

  document.addEventListener(
    "submit",
    (event) => {
      const form = event.target instanceof HTMLFormElement ? event.target : null;
      if (!form) return;
      if (lastSubmitClick.form === form && Date.now() - lastSubmitClick.at < 1500) return;
      const label = clean(form.getAttribute("aria-label") || form.getAttribute("name") || form.id || "form");
      send({ kind: "submit", target_label: label }, form);
    },
    true,
  );

  maskOnScreen();

  window.__peterRecorder = {
    stop() {
      active = false;
    },
    resume() {
      active = true;
      maskOnScreen();
    },
  };
})();
