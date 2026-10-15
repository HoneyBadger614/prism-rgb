// Prism Key bindings screen for the K65 keyboard and Nightsword mouse.
// Bindings live in the active profile (shared with lighting via Lighting.*).
// Uses `invoke`, `$` and `el` from app.js.
"use strict";

const Bindings = (() => {
  const BTN_LEFT = 0x110;
  const MODIFIERS = [[29, "Ctrl"], [42, "Shift"], [56, "Alt"], [125, "Super"]];
  const MEDIA = [[164, "Play / Pause"], [163, "Next track"], [165, "Previous track"], [166, "Stop"], [115, "Volume up"], [114, "Volume down"], [113, "Mute"]];
  const SIDE_BUTTONS = new Set(["Back", "Forward", "Sniper"]);

  let dev = "keyboard";          // "keyboard" or "mouse"
  let picked = null;             // the key or button being edited: its code
  let names = new Map();         // code -> name
  let apps = null;               // installed apps, loaded when first needed
  let appSearch = "";
  let capturing = false;
  let teaching = null;           // mouse button name being learned

  const store = () => Lighting.state().store;
  const list = () => Lighting.profile().bindings;
  const find = (code) => list().find((b) => b.device === dev && b.input === code);
  const nameOf = (code) => names.get(code) || `Key ${code}`;

  function learnNames() {
    const st = Lighting.state();
    for (const [, keys] of st.catalog) for (const [c, n] of keys) if (!names.has(c)) names.set(c, n);
  }

  // Fetch names for any codes Prism doesn't know yet (e.g. found by pressing).
  async function fetchNames() {
    const missing = new Set();
    for (const b of list()) {
      [b.input, ...(b.action.key ? [b.action.key] : []), ...(b.action.keys || [])].forEach((c) => names.has(c) || missing.add(c));
    }
    if (!missing.size) return;
    for (const [c, n] of await invoke("key_names", { codes: [...missing] })) names.set(c, n);
    render();
  }

  function describe(action) {
    switch (action.kind) {
      case "key": return nameOf(action.key);
      case "combo": return action.keys.map((k) => MODIFIERS.find(([c]) => c === k)?.[1] || nameOf(k)).join(" + ");
      case "launch": return `Opens ${apps?.find((a) => a.id === action.app)?.name || action.app}`;
      case "disabled": return "Does nothing";
    }
    return "";
  }

  function kindOf(action) {
    if (!action) return "normal";
    if (action.kind === "key" && MEDIA.some(([c]) => c === action.key)) return "media";
    return action.kind;
  }

  // The mouse's buttons with the code each sends now: its out-of-the-box
  // code, or what Prism learned from a press. Code 0 = not set up yet.
  function mouseButtons() {
    const learned = store().mouse_buttons || {};
    return Lighting.state().mouseButtons.map(([name, code]) => ({ name, code: learned[name] || code }));
  }

  // Name of the picked input, as the person sees it.
  function inputName(code) {
    if (dev === "keyboard") {
      const k = Lighting.state().keys.find((k) => k.code === code);
      if (k) return k.label || "Space";
    } else {
      const b = mouseButtons().find((b) => b.code === code);
      if (b) return `${b.name} button`;
    }
    return nameOf(code);
  }

  function setBinding(action) {
    const all = list();
    const i = all.findIndex((b) => b.device === dev && b.input === picked);
    if (action === null) {
      if (i >= 0) all.splice(i, 1);
    } else if (i >= 0) {
      all[i].action = action;
    } else {
      all.push({ device: dev, input: picked, action });
    }
    Lighting.save();
    render();
    refreshStatus();
  }

  async function refreshStatus() {
    // give the remapper a moment to start, then ask how it went
    await new Promise((r) => setTimeout(r, 150));
    const err = await invoke("bindings_status");
    $("bind-status").textContent = err || "";
  }

  /* ---------- picking a key or button ---------- */

  function renderPicker() {
    $("bind-pick-label").textContent = dev === "keyboard" ? "Pick a key" : "Pick a mouse button";
    $("bind-capture").textContent = dev === "keyboard" ? "Press a key to pick it" : "Press a mouse button";
    $("bind-kb").hidden = dev !== "keyboard";
    $("bind-mouse").hidden = dev !== "mouse";

    if (dev === "keyboard") {
      const seen = new Set();
      $("bind-kb").replaceChildren(...Lighting.state().keys.map((k) => {
        const b = el("button", "bkey");
        b.style.gridColumn = `span ${k.w}`;
        b.style.gridRow = String(k.row + 1);
        const bound = k.code && find(k.code);
        if (bound) b.classList.add("bound");
        if (k.code && k.code === picked && !seen.has(k.code)) b.classList.add("sel");
        b.append(el("span", null, k.label));
        if (bound && !seen.has(k.code)) b.append(el("span", "to", describe(bound.action)));
        if (k.code) seen.add(k.code);
        if (!k.code) {
          b.disabled = true;
          b.title = "Fn is handled by the keyboard itself";
        }
        b.addEventListener("click", () => pick(k.code));
        return b;
      }));
    } else {
      const card = ({ name, code }) => {
        const bound = code && find(code);
        const b = el("button", "mbtn" + (bound ? " bound" : "") + (code && code === picked ? " sel" : ""));
        b.append(el("span", "mbtn-name", name));
        if (name === "Left") {
          b.disabled = true;
          b.append(el("span", "mbtn-sends", "Always clicks"));
        } else if (teaching === name) {
          b.append(el("span", "mbtn-sends setup", "Press it now…"));
        } else if (!code) {
          b.append(el("span", "mbtn-sends setup", "Set up: press it once"));
        } else if (isStandard(name, code)) {
          b.append(el("span", "mbtn-sends", "Standard button"));
        } else {
          b.append(el("span", "mbtn-sends", `Sends ${nameOf(code)}`));
        }
        if (bound) b.append(el("span", "mbtn-to", `→ ${describe(bound.action)}`));
        b.addEventListener("click", () => (code ? pick(code) : teach(name)));
        return b;
      };
      const all = mouseButtons();
      $("bind-mouse-top").replaceChildren(...all.filter((b) => !SIDE_BUTTONS.has(b.name)).map(card));
      $("bind-mouse-side").replaceChildren(...all.filter((b) => SIDE_BUTTONS.has(b.name)).map(card));
    }
  }

  const isStandard = (name, code) => Lighting.state().mouseButtons.some(([n, c]) => n === name && c === code && c !== 0);

  // Learn what a mouse button sends by having the person press it.
  async function teach(name) {
    report(`teach ${name} (busy: ${capturing})`);
    if (capturing) return;
    capturing = true;
    teaching = name;
    showCapture(`Press the ${name} button on your Nightsword now…`);
    render();
    const got = await invoke("capture_input", { device: "mouse" });
    capturing = false;
    teaching = null;
    if (!got) {
      showCapture(`Nothing came from the ${name} button. It may only change something inside the mouse (like DPI), which Prism can't see yet.`);
      render();
      return;
    }
    if (got.code === BTN_LEFT) {
      showCapture("That was the left button. Try again and press the button you clicked.");
      render();
      return;
    }
    const other = mouseButtons().find((b) => b.code === got.code && b.name !== name);
    names.set(got.code, got.name);
    store().mouse_buttons = { ...(store().mouse_buttons || {}), [name]: got.code };
    Lighting.save();
    showCapture(other ? `Heads up: ${name} sends the same as ${other.name} (${got.name}), so they'll share a binding.` : "");
    pick(got.code, true);
  }

  function pick(code, keepMessage) {
    if (dev === "mouse" && code === BTN_LEFT) {
      showCapture("The left button can't be changed, so you can always click.");
      return;
    }
    picked = code;
    if (!keepMessage) showCapture("");
    render();
  }

  function showCapture(text) {
    $("capture-msg").hidden = !text;
    $("capture-msg").textContent = text;
  }

  // Wait for a real press on the device, then pick it (or use it as the
  // "act as" key when `forAction` is set).
  async function capture(forAction) {
    if (capturing) return;
    capturing = true;
    const from = forAction ? "keyboard" : dev;
    showCapture(from === "keyboard" ? "Press a key on your K65 now…" : "Press a button on your Nightsword now…");
    const got = await invoke("capture_input", { device: from });
    capturing = false;
    if (!got) {
      showCapture("Nothing was pressed. Try again when you're ready.");
      return;
    }
    names.set(got.code, got.name);
    if (forAction) {
      showCapture("");
      setBinding({ kind: "key", key: got.code });
    } else {
      pick(got.code);
    }
  }

  /* ---------- the editor ---------- */

  function keyOptions(select, chosen) {
    const st = Lighting.state();
    const groups = st.catalog.filter(([g]) => g !== "Media");
    select.replaceChildren(el("option", null, "Choose a key…"), ...groups.map(([g, keys]) => {
      const og = document.createElement("optgroup");
      og.label = g;
      og.append(...keys.map(([c, n]) => {
        const o = el("option", null, n);
        o.value = String(c);
        if (c === chosen) o.selected = true;
        return o;
      }));
      return og;
    }));
    select.firstChild.value = "";
    if (!chosen) select.value = "";
    // A captured key that isn't in the catalog still shows.
    if (chosen && select.value !== String(chosen)) {
      const o = el("option", null, nameOf(chosen));
      o.value = String(chosen);
      o.selected = true;
      select.append(o);
    }
  }

  function renderEditor() {
    $("bind-empty").hidden = picked !== null;
    $("bind-editor").hidden = picked === null;
    if (picked === null) return;
    const b = find(picked);
    const kind = kindOf(b?.action);
    $("bind-input-name").textContent = inputName(picked);
    $("bind-summary").textContent = b ? `Now: ${describe(b.action)}` : "Works as normal";
    document.querySelectorAll("[data-kind]").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.kind === kind)));

    $("bind-key-panel").hidden = kind !== "key";
    $("bind-combo-panel").hidden = kind !== "combo";
    $("bind-media-panel").hidden = kind !== "media";
    $("bind-app-panel").hidden = kind !== "launch";
    $("bind-disabled-note").hidden = kind !== "disabled";

    if (kind === "key") keyOptions($("bind-key-select"), b.action.key);

    if (kind === "combo") {
      const keys = b.action.keys;
      $("bind-mods").replaceChildren(...MODIFIERS.map(([c, n]) => {
        const on = keys.includes(c);
        const x = el("button", "chip-btn" + (on ? " on bound" : ""), n);
        x.setAttribute("aria-pressed", String(on));
        x.addEventListener("click", () => {
          const mods = MODIFIERS.map(([m]) => m).filter((m) => (m === c ? !on : keys.includes(m)));
          const main = keys.filter((k) => !MODIFIERS.some(([m]) => m === k));
          setBinding({ kind: "combo", keys: [...mods, ...main] });
        });
        return x;
      }));
      const main = keys.find((k) => !MODIFIERS.some(([m]) => m === k));
      keyOptions($("bind-combo-key"), main);
    }

    if (kind === "media") {
      $("bind-media").replaceChildren(...MEDIA.map(([c, n]) => {
        const x = el("button", "mini" + (b.action.key === c ? " on" : ""), n);
        if (b.action.key === c) x.style.borderColor = "var(--accent)";
        x.addEventListener("click", () => setBinding({ kind: "key", key: c }));
        return x;
      }));
    }

    if (kind === "launch") renderApps(b.action.app);
  }

  async function renderApps(chosen) {
    if (!apps) {
      $("bind-apps").replaceChildren(el("p", "hint", "Loading apps…"));
      apps = await invoke("list_apps");
    }
    const q = appSearch.trim().toLowerCase();
    const shown = apps.filter((a) => !q || a.name.toLowerCase().includes(q)).slice(0, 80);
    $("bind-apps").replaceChildren(...(shown.length ? shown.map((a) => {
      const x = el("button", "app-item" + (a.id === chosen ? " on" : ""), a.name);
      x.addEventListener("click", () => setBinding({ kind: "launch", app: a.id }));
      return x;
    }) : [el("p", "hint", "No apps match.")]));
  }

  function chooseKind(kind) {
    const current = find(picked)?.action;
    const keep = current && kindOf(current) === kind;
    if (keep) return;
    switch (kind) {
      case "normal": return setBinding(null);
      case "disabled": return setBinding({ kind: "disabled" });
      case "media": return setBinding({ kind: "key", key: 164 });
      case "key": return setBinding({ kind: "key", key: current?.kind === "key" ? current.key : picked === 58 ? 1 : 30 });
      case "combo": return setBinding({ kind: "combo", keys: [29, 31] });
      case "launch":
        // Nothing saved until an app is chosen; just show the list.
        renderAppsOnly();
        return;
    }
  }

  // Show the app list without saving a binding yet.
  function renderAppsOnly() {
    document.querySelectorAll("[data-kind]").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.kind === "launch")));
    ["bind-key-panel", "bind-combo-panel", "bind-media-panel"].forEach((id) => ($(id).hidden = true));
    $("bind-disabled-note").hidden = true;
    $("bind-app-panel").hidden = false;
    renderApps(null);
  }

  /* ---------- list of bindings ---------- */

  function renderList() {
    const mine = list().filter((b) => b.device === dev);
    $("bind-count").textContent = mine.length ? `${mine.length} on this ${dev}` : "";
    $("bind-list").replaceChildren(...(mine.length ? mine.map((b) => {
      const row = el("div", "layer-row" + (b.input === picked ? " active" : ""));
      row.tabIndex = 0;
      row.setAttribute("role", "button");
      const text = el("span", "layer-text");
      text.append(el("span", "layer-name", inputName(b.input)), el("span", "layer-meta", describe(b.action)));
      const del = el("button", "icon-btn");
      del.innerHTML = '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>';
      del.title = "Remove binding";
      del.setAttribute("aria-label", `Remove binding for ${inputName(b.input)}`);
      del.addEventListener("click", (e) => {
        e.stopPropagation();
        picked = b.input;
        setBinding(null);
      });
      row.append(el("span", "bind-row-arrow", "→"), text, del);
      row.addEventListener("click", () => pick(b.input));
      row.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(b.input); } });
      return row;
    }) : [el("p", "hint", `No bindings on this ${dev} yet. Everything works as normal.`)]));
  }

  function render() {
    if (!Lighting.state()) return;
    $("bindings-on").checked = store().bindings_on;
    renderPicker();
    renderEditor();
    renderList();
  }

  function show(which) {
    if (which !== dev) {
      dev = which;
      picked = null;
      showCapture("");
    }
    learnNames();
    render();
    fetchNames();
    refreshStatus();
  }

  function init() {
    $("bindings-on").addEventListener("change", (e) => {
      store().bindings_on = e.target.checked;
      Lighting.save();
      refreshStatus();
    });
    $("bind-capture").addEventListener("click", () => capture(false));
    $("bind-key-capture").addEventListener("click", () => capture(true));
    $("bind-key-select").addEventListener("change", (e) => e.target.value && setBinding({ kind: "key", key: Number(e.target.value) }));
    $("bind-combo-key").addEventListener("change", (e) => {
      if (!e.target.value) return;
      const mods = find(picked).action.keys.filter((k) => MODIFIERS.some(([m]) => m === k));
      setBinding({ kind: "combo", keys: [...mods, Number(e.target.value)] });
    });
    document.querySelectorAll("[data-kind]").forEach((x) => x.addEventListener("click", () => chooseKind(x.dataset.kind)));
    $("bind-app-search").addEventListener("input", (e) => {
      appSearch = e.target.value;
      renderApps(find(picked)?.action.app);
    });
    Lighting.onChange(() => { if (!$("bindings-view").hidden) render(); });
  }

  return { init, show };
})();
