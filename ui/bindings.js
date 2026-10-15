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
      case "dpi": return DPI_OPS[action.op];
      case "macro": {
        const n = action.steps.filter((s) => s.kind === "down").length;
        const how = { once: "once", repeat: "while held", toggle: "on/off" }[action.mode];
        return `Macro: ${n} key${n === 1 ? "" : "s"}, ${how}`;
      }
      case "disabled": return "Does nothing";
    }
    return "";
  }

  const DPI_OPS = { up: "DPI up", down: "DPI down", cycle: "Cycle DPI stages", sniper: "Sniper DPI while held" };

  function kindOf(action) {
    if (!action) return "normal";
    if (action.kind === "key" && MEDIA.some(([c]) => c === action.key)) return "media";
    return action.kind;
  }

  // The mouse's buttons and the code bindings use for each. The first five
  // are standard Linux buttons; the rest the mouse normally handles itself.
  function mouseButtons() {
    return Lighting.state().mouseButtons.map(([name, code], i) => ({ name, code, standard: i < 5 }));
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
      const card = ({ name, code, standard }) => {
        const bound = find(code);
        const b = el("button", "mbtn" + (bound ? " bound" : "") + (code === picked ? " sel" : ""));
        b.append(el("span", "mbtn-name", name));
        if (name === "Left") {
          b.disabled = true;
          b.append(el("span", "mbtn-sends", "Always clicks"));
        } else if (bound) {
          b.append(el("span", "mbtn-to", `→ ${describe(bound.action)}`));
        } else {
          b.append(el("span", "mbtn-sends", standard ? "Standard button" : "Your mouse's own setting"));
        }
        b.addEventListener("click", () => pick(code));
        return b;
      };
      const all = mouseButtons();
      $("bind-mouse-top").replaceChildren(...all.filter((b) => !SIDE_BUTTONS.has(b.name)).map(card));
      $("bind-mouse-side").replaceChildren(...all.filter((b) => SIDE_BUTTONS.has(b.name)).map(card));
    }
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
    $("bind-dpi-panel").hidden = kind !== "dpi";
    $("bind-macro-panel").hidden = kind !== "macro";
    if (kind === "macro") renderMacro(b.action);
    if (kind === "dpi") {
      document.querySelectorAll("[data-op]").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.op === b.action.op)));
      $("bind-dpi-note").textContent = Lighting.profile().dpi.enabled
        ? ""
        : "Turn on “Let Prism control DPI” on the mouse's Key bindings tab for this to work.";
    }
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
      case "dpi": return setBinding({ kind: "dpi", op: picked === 0x2f2 ? "sniper" : picked === 0x2f1 ? "down" : "up" });
      case "macro": return setBinding({ kind: "macro", mode: "once", steps: [] });
      case "launch":
        // Nothing saved until an app is chosen; just show the list.
        renderAppsOnly();
        return;
    }
  }

  // Show the app list without saving a binding yet.
  function renderAppsOnly() {
    document.querySelectorAll("[data-kind]").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.kind === "launch")));
    ["bind-key-panel", "bind-combo-panel", "bind-media-panel", "bind-dpi-panel", "bind-macro-panel"].forEach((id) => ($(id).hidden = true));
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

  /* ---------- macros ---------- */

  let recording = null; // { started, timer } while recording

  function renderMacro(action) {
    document.querySelectorAll("[data-mode]").forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.mode === action.mode)));
    const steps = action.steps;
    const icon = (svg) => {
      const b = el("button", "icon-btn");
      b.innerHTML = svg;
      return b;
    };
    $("macro-steps").replaceChildren(...(steps.length ? steps.map((s, i) => {
      const row = el("div", "mstep" + (s.kind === "wait" ? " wait" : ""));
      const what = el("span", "what");
      if (s.kind === "wait") {
        const ms = el("input", "text-input");
        ms.type = "number";
        ms.min = "0";
        ms.max = "10000";
        ms.value = String(s.ms);
        ms.setAttribute("aria-label", "Wait in milliseconds");
        ms.addEventListener("change", () => {
          s.ms = Math.max(0, Math.min(10000, Number(ms.value) || 0));
          setBinding(action);
        });
        what.append(el("span", "tag", "Wait"), ms, el("span", "hint", "ms"));
      } else {
        what.append(el("span", "tag", s.kind === "down" ? "Press" : "Release"), el("span", null, nameOf(s.key)));
      }
      const up = icon('<svg viewBox="0 0 24 24"><path d="M12 19V5M6 11l6-6 6 6"/></svg>');
      up.title = "Move up";
      up.disabled = i === 0;
      up.addEventListener("click", () => { [steps[i - 1], steps[i]] = [steps[i], steps[i - 1]]; setBinding(action); });
      const down = icon('<svg viewBox="0 0 24 24"><path d="M12 5v14M6 13l6 6 6-6"/></svg>');
      down.title = "Move down";
      down.disabled = i === steps.length - 1;
      down.addEventListener("click", () => { [steps[i + 1], steps[i]] = [steps[i], steps[i + 1]]; setBinding(action); });
      const del = icon('<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>');
      del.title = "Delete step";
      del.addEventListener("click", () => { steps.splice(i, 1); setBinding(action); });
      [up, down, del].forEach((b) => b.setAttribute("aria-label", b.title));
      row.append(what, up, down, del);
      return row;
    }) : [el("p", "hint", "No steps yet. Press Record, or add key taps and waits below.")]));

    keyOptions($("macro-key"), null);
    const keys = steps.filter((s) => s.kind === "down").length;
    const ms = steps.reduce((t, s) => t + (s.kind === "wait" ? s.ms : 0), 0);
    $("macro-summary").textContent = steps.length ? `${steps.length} steps · ${keys} key presses · about ${(ms / 1000).toFixed(1)} s` : "";
    $("macro-rec").classList.toggle("on", !!recording);
    $("macro-rec").textContent = recording ? "Stop" : "Record";
  }

  function currentMacro() {
    const a = find(picked)?.action;
    return a && a.kind === "macro" ? a : null;
  }

  async function toggleRecording() {
    if (recording) return stopRecording();
    await invoke("macro_record_start");
    recording = { started: Date.now() };
    recording.timer = setInterval(() => {
      const s = Math.floor((Date.now() - recording.started) / 1000);
      $("macro-rec-msg").textContent = `Recording… ${s}s. Press your keys, then Stop.`;
      if (s >= 60) stopRecording();
    }, 250);
    $("macro-rec-msg").textContent = "Recording… press your keys, then Stop.";
    renderEditor();
  }

  async function stopRecording() {
    if (!recording) return;
    clearInterval(recording.timer);
    recording = null;
    const steps = await invoke("macro_record_stop");
    $("macro-rec-msg").textContent = steps.length ? `Recorded ${steps.length} steps.` : "Nothing was recorded.";
    const m = currentMacro();
    if (m && steps.length) {
      m.steps = steps;
      setBinding(m);
      fetchNames();
    } else {
      renderEditor();
    }
  }

  /* ---------- DPI ---------- */

  function renderDpi() {
    $("dpi-card").hidden = dev !== "mouse";
    if (dev !== "mouse") return;
    const d = Lighting.profile().dpi;
    $("dpi-on").checked = d.enabled;
    $("dpi-card").classList.toggle("off", !d.enabled);
    $("dpi-off-note").hidden = d.enabled;
    if (!d.enabled) $("dpi-now").textContent = "";
    const slider = (id, v) => {
      $(id).value = v;
      $(id).style.setProperty("--fill", `${((v - 100) / 17900) * 100}%`);
      $(`${id}-val`).textContent = `${v} DPI`;
    };
    d.stages.forEach((v, i) => slider(`dpi-${i + 1}`, v));
    slider("dpi-sniper", d.sniper);
    document.querySelectorAll("[data-start]").forEach((x) => x.setAttribute("aria-pressed", String(Number(x.dataset.start) === d.start)));
  }

  function editDpi(changes) {
    Object.assign(Lighting.profile().dpi, changes);
    Lighting.save();
    renderDpi();
    renderEditor();
  }

  function render() {
    if (!Lighting.state()) return;
    $("bindings-on").checked = store().bindings_on;
    renderDpi();
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

    document.querySelectorAll("[data-op]").forEach((x) => x.addEventListener("click", () => setBinding({ kind: "dpi", op: x.dataset.op })));

    document.querySelectorAll("#macro-modes [data-mode]").forEach((x) => x.addEventListener("click", () => {
      const m = currentMacro();
      if (m) setBinding({ ...m, mode: x.dataset.mode });
    }));
    $("macro-rec").addEventListener("click", toggleRecording);
    $("macro-add-tap").addEventListener("click", () => {
      const m = currentMacro();
      const key = Number($("macro-key").value);
      if (!m || !key) return;
      m.steps.push({ kind: "down", key }, { kind: "wait", ms: 20 }, { kind: "up", key });
      setBinding(m);
    });
    $("macro-add-wait").addEventListener("click", () => {
      const m = currentMacro();
      if (!m) return;
      m.steps.push({ kind: "wait", ms: 100 });
      setBinding(m);
    });
    $("macro-no-waits").addEventListener("click", () => {
      const m = currentMacro();
      if (!m) return;
      m.steps = m.steps.filter((s) => s.kind !== "wait");
      setBinding(m);
    });
    $("dpi-on").addEventListener("change", (e) => editDpi({ enabled: e.target.checked }));
    // Sliders show the number while dragging and send it to the mouse on release.
    const live = (id) => $(id).addEventListener("input", (e) => {
      $(`${id}-val`).textContent = `${e.target.value} DPI`;
      $(id).style.setProperty("--fill", `${((e.target.value - 100) / 17900) * 100}%`);
    });
    [1, 2, 3].forEach((n) => {
      live(`dpi-${n}`);
      $(`dpi-${n}`).addEventListener("change", (e) => {
        const stages = [...Lighting.profile().dpi.stages];
        stages[n - 1] = Number(e.target.value);
        editDpi({ stages });
      });
    });
    live("dpi-sniper");
    $("dpi-sniper").addEventListener("change", (e) => editDpi({ sniper: Number(e.target.value) }));
    document.querySelectorAll("[data-start]").forEach((x) => x.addEventListener("click", () => editDpi({ start: Number(x.dataset.start) })));
    window.__TAURI__.event.listen("dpi-status", (e) => {
      const { stage, dpi } = e.payload;
      $("dpi-now").textContent = stage === null ? "" : `Now: ${stage === 0 ? "sniper" : `stage ${stage}`} · ${dpi} DPI`;
    });
  }

  return { init, show };
})();
