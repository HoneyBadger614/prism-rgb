// Prism lighting screen for the Lighting Node Pro and the K65 keyboard:
// profiles, layers, LED selection and painting, the effect picker, and the
// layer editor. The same screen serves both; `dev` says which is showing.
// Uses `invoke`, `$` and `el` from app.js.
"use strict";

const Lighting = (() => {
  const PURPLE = "#7c3aed", PINK = "#ff2d95", WHITE = "#ffffff";

  const QUICK_COLORS = [
    ["Ember purple", PURPLE], ["Hot pink", PINK], ["White", WHITE], ["Red", "#ff1a1a"],
    ["Orange", "#ff7a00"], ["Green", "#22e55b"], ["Cyan", "#22d3ee"], ["Blue", "#2d5bff"],
  ];

  // Every effect: what it's called, what it does, which controls it uses
  // (c1/c2 are color labels; missing = not used), and its starting settings.
  const EFFECTS = {
    static: { name: "Static", desc: "One solid color", c1: "Color", start: { color1: PURPLE } },
    gradient: { name: "Gradient", desc: "One color fading into another", c1: "From", c2: "To", spread: true, start: { color1: PURPLE, color2: PINK } },
    wave: { name: "Wave", desc: "A gradient that keeps flowing", c1: "Color 1", c2: "Color 2", spread: true, speed: true, start: { color1: PURPLE, color2: PINK } },
    breathe: { name: "Breathe", desc: "Slowly fades in and out", c1: "Color 1", c2: "Color 2", speed: true, start: { color1: PURPLE, color2: PINK, speed: 3 } },
    rainbow: { name: "Rainbow", desc: "Every color, flowing", spread: true, speed: true, start: { speed: 4 } },
    "color-cycle": { name: "Color cycle", desc: "Everything drifts through every color together", speed: true, start: { speed: 4 } },
    comet: { name: "Comet", desc: "A bright head with a fading tail", c1: "Head", c2: "Background", spread: true, speed: true, start: { color1: PINK, color2: "#140624", spread: "per-fan" } },
    strobe: { name: "Strobe", desc: "Fast flashing", c1: "Flash", c2: "Between", speed: true, start: { color1: WHITE, color2: "#000000", speed: 4 } },
  };

  const ICON = {
    eye: '<svg viewBox="0 0 24 24"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>',
    eyeOff: '<svg viewBox="0 0 24 24"><path d="M3 3l18 18M10.6 5.1A10.8 10.8 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4.2M6.6 6.6C3.9 8.4 2 12 2 12s3.5 7 10 7c1.7 0 3.2-.5 4.5-1.1"/></svg>',
    up: '<svg viewBox="0 0 24 24"><path d="M12 19V5M6 11l6-6 6 6"/></svg>',
    down: '<svg viewBox="0 0 24 24"><path d="M12 5v14M6 13l6 6 6-6"/></svg>',
    trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>',
    check: '<svg viewBox="0 0 24 24"><path d="M5 12l5 5 9-10"/></svg>',
  };

  let light = null;          // { store, layout (fans), keys (keyboard) }
  let frame = { node: [], keyboard: [] }; // latest colors sent to each device
  let dev = "node";          // which device is showing: "node" or "keyboard"
  let selection = new Set(); // "channel:index"
  let layerIndex = 0;        // layer being edited, in the active profile
  let activeSlot = "color1";
  let hsv = null;            // hue/sat/value of the color being edited
  let sendQueued = false;
  let spots = [];            // where each LED was drawn, for clicking
  let drag = null;           // box selection in progress
  let pickerMode = "add";    // effect picker: "add" a layer, or "change" this layer's effect
  let renaming = false;      // renaming the profile in the menu
  let deleteArmed = null;

  /* ---------- state helpers ---------- */

  const profile = () => light.store.profiles[light.store.active];
  // The showing device's layers in the active profile.
  const stack = () => profile()[dev === "node" ? "layers" : "keyboard"];
  const layer = () => stack()[layerIndex];
  const key = (c, i) => `${c}:${i}`;
  const ref = (k) => k.split(":").map(Number);

  function allLeds() {
    if (dev === "keyboard") return light.keys.map((_, i) => key(0, i));
    const out = [];
    light.layout.forEach((ch, c) => {
      for (let i = 0; i < ch.fans * (ch.inner + ch.outer); i++) out.push(key(0 + c, i));
    });
    return out;
  }

  const MODIFIERS = new Set(["Tab", "Caps", "Shift", "Ctrl", "Win", "Alt", "Fn", "Menu"]);

  // Named groups of keys for the keyboard.
  function keyGroups() {
    const pick = (test) => light.keys.flatMap((k, i) => (test(k) ? [key(0, i)] : []));
    return [
      ["All", allLeds()],
      ["WASD", pick((k) => ["W", "A", "S", "D"].includes(k.label))],
      ["Letters", pick((k) => /^[A-Z]$/.test(k.label))],
      ["Number row", pick((k) => k.row === 0 && /^[0-9]$/.test(k.label))],
      ["Modifiers", pick((k) => MODIFIERS.has(k.label))],
      ["Space bar", pick((k) => k.row === 4 && (k.label === "Space" || k.label === ""))],
      ["Esc", pick((k) => k.label === "Esc")],
    ];
  }

  // Named groups: All, Hubs, Rings, Fan 1, Fan 2... (or key groups).
  function groups() {
    if (dev === "keyboard") return keyGroups();
    const g = [["All", allLeds()], ["Hubs", []], ["Rings", []]];
    let fanNo = 0;
    light.layout.forEach((ch, c) => {
      const per = ch.inner + ch.outer;
      for (let f = 0; f < ch.fans; f++) {
        const fan = [];
        for (let j = 0; j < per; j++) {
          const k = key(c, f * per + j);
          fan.push(k);
          (j < ch.inner ? g[1][1] : g[2][1]).push(k);
        }
        g.push([`Fan ${++fanNo}`, fan]);
      }
    });
    return g;
  }

  const group = (name) => groups().find(([n]) => n === name)[1];
  const ledsTarget = (keys) => ({ kind: "leds", leds: keys.map(ref) });
  const sameSet = (a, b) => a.length === b.length && a.every((k) => b.includes(k));

  function describeTarget(target) {
    if (target.kind === "all") return dev === "keyboard" ? "All keys" : "All LEDs";
    const leds = target.leds.map(([c, i]) => key(c, i));
    if (!leds.length) return "No LEDs";
    for (const [name, set] of groups()) if (sameSet(leds, set)) return name === "All" ? describeTarget({ kind: "all" }) : name;
    return `${leds.length} ${unit(leds.length)}`;
  }

  const unit = (n) => (dev === "keyboard" ? (n === 1 ? "key" : "keys") : n === 1 ? "LED" : "LEDs");

  function send() {
    // Coalesce rapid changes (slider drags) into one update per screen refresh.
    if (sendQueued) return;
    sendQueued = true;
    requestAnimationFrame(async () => {
      sendQueued = false;
      await invoke("set_lighting", { store: light.store });
    });
  }

  function changed() {
    renderAll();
    send();
  }

  function editLayer(changes) {
    Object.assign(layer(), changes);
    changed();
  }

  function newLayer(effect, fields) {
    return {
      name: EFFECTS[effect].name, enabled: true, effect, color1: PURPLE, color2: PINK,
      spread: "across", brightness: 100, speed: 5, target: { kind: "all" },
      ...EFFECTS[effect].start, ...fields,
    };
  }

  function pushLayers(layers) {
    const list = stack();
    for (const l of layers) if (list.length < 32) list.push(l);
    layerIndex = list.length - 1;
    activeSlot = "color1";
    hsv = null;
    changed();
  }

  // Ready-made multi-layer looks for the keyboard.
  const KEY_COMBOS = [
    {
      name: "WASD glow", desc: "Ember fade with bright white WASD keys",
      thumb: `radial-gradient(circle, ${WHITE} 0 18%, transparent 20%), linear-gradient(90deg, ${PURPLE}, ${PINK})`,
      layers: () => [
        newLayer("gradient", { name: "Ember fade" }),
        newLayer("static", { name: "WASD", color1: WHITE, target: ledsTarget(group("WASD")) }),
      ],
    },
    {
      name: "Ember rows", desc: "Purple at the top fading to pink at the bottom",
      thumb: `linear-gradient(180deg, ${PURPLE}, ${PINK})`,
      layers: () => [newLayer("gradient", { name: "Ember rows", spread: "per-fan" })],
    },
    {
      name: "Rainbow & pink keys", desc: "Rainbow wave with hot pink modifiers",
      thumb: `linear-gradient(90deg, ${PINK} 0 15%, transparent 15% 85%, ${PINK} 85%), linear-gradient(90deg,#f00,#ff0,#0f0,#0ff,#00f,#f0f)`,
      layers: () => [
        newLayer("rainbow", { name: "Rainbow" }),
        newLayer("static", { name: "Modifiers", color1: PINK, target: ledsTarget(group("Modifiers")) }),
      ],
    },
  ];

  // Ready-made multi-layer looks for the fans. Each returns the layers to add.
  const COMBOS = [
    {
      name: "Hub & Ring", desc: "White hubs, flowing purple and pink rings",
      thumb: `radial-gradient(circle, ${WHITE} 0 22%, transparent 24%), linear-gradient(90deg, ${PURPLE}, ${PINK})`,
      layers: () => [
        newLayer("wave", { name: "Rings", spread: "per-fan" }),
        newLayer("static", { name: "Hubs", color1: WHITE, target: ledsTarget(group("Hubs")) }),
      ],
    },
    {
      name: "Ember duo", desc: "Purple rings with hot pink hubs",
      thumb: `radial-gradient(circle, ${PINK} 0 22%, transparent 24%), ${PURPLE}`,
      layers: () => [
        newLayer("static", { name: "Rings", color1: PURPLE }),
        newLayer("static", { name: "Hubs", color1: PINK, target: ledsTarget(group("Hubs")) }),
      ],
    },
    {
      name: "Fan by fan", desc: "Each fan gets its own color",
      thumb: `linear-gradient(90deg, ${PURPLE} 0 33%, ${PINK} 33% 66%, ${WHITE} 66%)`,
      layers: () => groups().filter(([n]) => n.startsWith("Fan ")).map(([n, set], i) =>
        newLayer("static", { name: n, color1: [PURPLE, PINK, WHITE, "#22d3ee", "#ff7a00"][i % 5], target: ledsTarget(set) })),
    },
    {
      name: "Comet & rainbow hubs", desc: "Comets racing round the rings, rainbow hubs",
      thumb: `radial-gradient(circle, #ff0 0 10%, #0ff 14%, #f0f 20%, transparent 24%), linear-gradient(90deg, #140624 40%, ${PINK})`,
      layers: () => [
        newLayer("comet", { name: "Comets" }),
        newLayer("rainbow", { name: "Rainbow hubs", spread: "per-fan", target: ledsTarget(group("Hubs")) }),
      ],
    },
  ];

  /* ---------- profiles (top bar menu) ---------- */

  function renderProfiles() {
    $("profile-name").textContent = profile().name;
    const list = $("profile-list");
    list.replaceChildren(...light.store.profiles.map((p, i) => {
      const active = i === light.store.active;
      if (active && renaming) {
        const input = el("input", "text-input menu-rename");
        input.value = p.name;
        input.maxLength = 40;
        input.setAttribute("aria-label", "Profile name");
        const done = (save) => {
          if (!renaming) return;
          renaming = false;
          if (save && input.value.trim()) p.name = input.value.trim();
          changed();
        };
        input.addEventListener("keydown", (e) => {
          e.stopPropagation();
          if (e.key === "Enter") done(true);
          if (e.key === "Escape") done(false);
        });
        input.addEventListener("blur", () => done(true));
        requestAnimationFrame(() => { input.focus(); input.select(); });
        return input;
      }
      const b = el("button", "menu-item profile-item" + (active ? " active" : ""));
      b.setAttribute("role", "menuitemradio");
      b.setAttribute("aria-checked", String(active));
      const check = el("span", "check");
      if (active) check.innerHTML = ICON.check;
      b.append(check, el("span", null, p.name));
      b.addEventListener("click", () => {
        light.store.active = i;
        layerIndex = Math.max(0, stack().length - 1);
        hsv = null;
        closeMenu();
        changed();
      });
      return b;
    }));
    const del = $("profile-delete");
    del.disabled = light.store.profiles.length <= 1;
    del.textContent = deleteArmed ? "Click again to delete" : "Delete";
  }

  function openMenu() {
    $("profile-menu").hidden = false;
    $("profile-btn").setAttribute("aria-expanded", "true");
  }

  function closeMenu() {
    $("profile-menu").hidden = true;
    $("profile-btn").setAttribute("aria-expanded", "false");
    renaming = false;
  }

  function addProfile(p) {
    if (light.store.profiles.length >= 50) return;
    light.store.profiles.splice(light.store.active + 1, 0, p);
    light.store.active += 1;
    layerIndex = Math.max(0, stack().length - 1);
    hsv = null;
    changed();
  }

  /* ---------- effect picker ---------- */

  // CSS background that shows an effect, using the given colors.
  function thumbCss(effect, c1, c2) {
    switch (effect) {
      case "static": return c1;
      case "gradient": case "wave": return `linear-gradient(90deg, ${c1}, ${c2}, ${c1})`;
      case "breathe": return `linear-gradient(90deg, ${c1}, ${c2})`;
      case "rainbow": case "color-cycle": return "linear-gradient(90deg,#f00,#ff0,#0f0,#0ff,#00f,#f0f,#f00)";
      case "comet": return `linear-gradient(90deg, ${c2} 0 45%, ${c1} 70%, ${c2} 72%)`;
      case "strobe": return c1;
    }
    return c1;
  }

  function thumb(effect, c1, c2) {
    const t = el("span", `fx-thumb fx-${effect}`);
    t.style.background = thumbCss(effect, c1, c2);
    if (effect === "strobe") t.style.setProperty("--off", c2);
    return t;
  }

  function openPicker(mode) {
    pickerMode = mode;
    const l = layer();
    $("fx-title").textContent = mode === "add" ? "Add effect" : "Change effect";
    $("fx-sub").textContent = mode === "add"
      ? `Adds a new layer on top, lit on ${selection.size ? `your ${selection.size} selected ${unit(selection.size)}` : `all ${unit(2)}`}.`
      : `For the layer “${l.name}”. Its colors and LEDs stay the same.`;
    $("fx-combos-wrap").hidden = mode !== "add";

    $("fx-effects").replaceChildren(...Object.entries(EFFECTS).map(([k, e]) => {
      const card = el("button", "fx-card" + (mode === "change" && l.effect === k ? " current" : ""));
      const c1 = mode === "change" && e.c1 ? l.color1 : e.start.color1 || PURPLE;
      const c2 = mode === "change" && e.c2 ? l.color2 : e.start.color2 || PINK;
      card.append(thumb(k, c1, c2), el("span", "fx-name", e.name), el("span", "fx-desc", e.desc));
      card.addEventListener("click", () => {
        closePicker();
        if (pickerMode === "change") {
          const keep = { effect: k, name: l.name === EFFECTS[l.effect].name ? e.name : l.name };
          if (k === "comet" || k === "strobe") Object.assign(keep, e.start, { color1: l.color1 });
          if (!e.c2 && activeSlot === "color2") activeSlot = "color1";
          hsv = null;
          editLayer(keep);
        } else {
          const target = selection.size ? ledsTarget([...selection]) : { kind: "all" };
          pushLayers([newLayer(k, { target })]);
        }
      });
      return card;
    }));

    $("fx-combos").replaceChildren(...(dev === "keyboard" ? KEY_COMBOS : COMBOS).map((c) => {
      const card = el("button", "fx-card");
      const t = el("span", "fx-thumb");
      t.style.background = c.thumb;
      card.append(t, el("span", "fx-name", c.name), el("span", "fx-desc", c.desc));
      card.addEventListener("click", () => { closePicker(); pushLayers(c.layers()); });
      return card;
    }));

    $("fx-overlay").hidden = false;
    requestAnimationFrame(() => $("fx-effects").querySelector(".fx-card")?.focus());
  }

  function closePicker() {
    $("fx-overlay").hidden = true;
  }

  /* ---------- layers ---------- */

  function renderLayers() {
    const layers = stack();
    layerIndex = Math.min(layerIndex, Math.max(0, layers.length - 1));
    const rows = [];
    // Show the top layer (painted last) first.
    for (let i = layers.length - 1; i >= 0; i--) {
      const l = layers[i];
      const fx = EFFECTS[l.effect];
      const row = el("div", "layer-row" + (i === layerIndex ? " active" : "") + (l.enabled ? "" : " hidden-layer"));
      row.tabIndex = 0;
      row.setAttribute("role", "button");
      row.setAttribute("aria-label", `Edit layer ${l.name}`);

      const eye = el("button", "icon-btn");
      eye.innerHTML = l.enabled ? ICON.eye : ICON.eyeOff;
      eye.title = l.enabled ? "Hide layer" : "Show layer";
      eye.setAttribute("aria-label", eye.title);
      eye.addEventListener("click", (e) => { e.stopPropagation(); l.enabled = !l.enabled; changed(); });

      const sw = thumb(l.effect, l.color1, l.color2);
      sw.classList.add("layer-swatch");

      const text = el("span", "layer-text");
      text.append(el("span", "layer-name", l.name), el("span", "layer-meta", `${fx.name} · ${describeTarget(l.target)}`));

      const tools = el("span", "layer-tools");
      const tool = (icon, title, fn, disabled) => {
        const b = el("button", "icon-btn");
        b.innerHTML = icon;
        b.title = title;
        b.setAttribute("aria-label", title);
        b.disabled = disabled;
        b.addEventListener("click", (e) => { e.stopPropagation(); fn(); });
        return b;
      };
      tools.append(
        tool(ICON.up, "Move up", () => { [layers[i], layers[i + 1]] = [layers[i + 1], layers[i]]; layerIndex = i + 1; changed(); }, i === layers.length - 1),
        tool(ICON.down, "Move down", () => { [layers[i], layers[i - 1]] = [layers[i - 1], layers[i]]; layerIndex = i - 1; changed(); }, i === 0),
        tool(ICON.trash, "Delete layer", () => { layers.splice(i, 1); layerIndex = Math.min(layerIndex, layers.length - 1); hsv = null; changed(); }, false),
      );

      row.append(eye, sw, text, tools);
      const pick = () => {
        layerIndex = i;
        hsv = null;
        if (!fx.c2 && activeSlot === "color2") activeSlot = "color1";
        renderAll();
      };
      row.addEventListener("click", pick);
      row.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(); } });
      rows.push(row);
    }
    if (!rows.length) rows.push(el("p", "hint", "No effects yet. Press Add effect to start."));
    $("layer-list").replaceChildren(...rows);
    $("layer-add").disabled = layers.length >= 32;
  }

  /* ---------- selection ---------- */

  function renderSelection() {
    const chips = groups().map(([name, set]) => {
      const b = el("button", "chip-btn" + (selection.size && sameSet([...selection], set) ? " on" : ""), name);
      b.addEventListener("click", () => { selection = new Set(set); renderAll(); });
      return b;
    });
    const clear = el("button", "chip-btn", "Clear");
    clear.disabled = !selection.size;
    clear.addEventListener("click", () => { selection.clear(); renderAll(); });
    $("select-chips").replaceChildren(...chips, clear);
    $("sel-count").textContent = selection.size ? `${selection.size} selected` : "";
    $("paint-bar").hidden = !selection.size;
    $("target-sel").disabled = !selection.size;
    drawFans();
  }

  function paint(hex) {
    const n = stack().filter((l) => l.name.startsWith("Paint")).length + 1;
    pushLayers([newLayer("static", { name: `Paint ${n}`, color1: hex, target: ledsTarget([...selection]) })]);
  }

  function hitTest(x, y) {
    if (dev === "keyboard") {
      return spots.find((s) => x >= s.rect[0] && x <= s.rect[0] + s.rect[2] && y >= s.rect[1] && y <= s.rect[1] + s.rect[3]) || null;
    }
    let best = null, bestD = 18 * 18;
    for (const s of spots) {
      const d = (s.x - x) ** 2 + (s.y - y) ** 2;
      if (d < bestD) { bestD = d; best = s; }
    }
    return best;
  }

  function initCanvas() {
    const canvas = $("fans");
    const pos = (e) => {
      const r = canvas.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    // Stop the web view from starting a text selection or image drag, so a
    // press-drag-release is one box selection.
    canvas.addEventListener("dragstart", (e) => e.preventDefault());
    canvas.addEventListener("selectstart", (e) => e.preventDefault());
    canvas.addEventListener("mousedown", (e) => e.preventDefault());
    canvas.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      canvas.setPointerCapture(e.pointerId);
      const p = pos(e);
      drag = { x0: p.x, y0: p.y, x1: p.x, y1: p.y, add: e.shiftKey || e.ctrlKey || e.metaKey, moved: false };
    });
    canvas.addEventListener("pointermove", (e) => {
      const p = pos(e);
      if (!drag) {
        canvas.style.cursor = hitTest(p.x, p.y) ? "pointer" : "crosshair";
        return;
      }
      drag.x1 = p.x; drag.y1 = p.y;
      if (Math.abs(drag.x1 - drag.x0) + Math.abs(drag.y1 - drag.y0) > 6) drag.moved = true;
      if (drag.moved) drawFans();
    });
    const finish = () => {
      if (!drag) return;
      const d = drag;
      drag = null;
      if (d.moved) {
        const [xa, xb] = [Math.min(d.x0, d.x1), Math.max(d.x0, d.x1)];
        const [ya, yb] = [Math.min(d.y0, d.y1), Math.max(d.y0, d.y1)];
        if (!d.add) selection.clear();
        for (const s of spots) if (s.x >= xa && s.x <= xb && s.y >= ya && s.y <= yb) selection.add(s.key);
      } else {
        const hit = hitTest(d.x0, d.y0);
        if (d.add) {
          if (hit) selection.has(hit.key) ? selection.delete(hit.key) : selection.add(hit.key);
        } else if (hit) {
          const only = selection.size === 1 && selection.has(hit.key);
          selection = only ? new Set() : new Set([hit.key]);
        } else {
          selection.clear();
        }
      }
      renderAll();
    };
    canvas.addEventListener("pointerup", finish);
    canvas.addEventListener("pointercancel", () => { drag = null; drawFans(); });
  }

  /* ---------- preview ---------- */

  function drawFans() {
    const canvas = $("fans");
    if (!light || canvas.offsetParent === null) return;
    if (dev === "keyboard") return drawKeyboard(canvas);
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const fans = [];
    light.layout.forEach((ch, c) => {
      const per = ch.inner + ch.outer;
      for (let f = 0; f < ch.fans; f++) fans.push({ c, base: f * per, inner: ch.inner, outer: ch.outer });
    });
    spots = [];
    if (!fans.length) return;

    const css = getComputedStyle(document.documentElement);
    const body = css.getPropertyValue("--surface").trim();
    const edge = css.getPropertyValue("--line").trim();
    const raised = css.getPropertyValue("--raised").trim();
    const accent = css.getPropertyValue("--accent").trim();
    const slot = w / fans.length;
    const R = Math.min(slot, h) * 0.44;
    const marks = [];

    // A ring of glowing light segments; remembers where each LED is.
    const ring = (fan, cx, cy, radius, width, first, count) => {
      const step = (Math.PI * 2) / count;
      for (let k = 0; k < count; k++) {
        const i = fan.base + first + k;
        const color = (frame.node[fan.c] || [])[i] || "#000000";
        const a = -Math.PI / 2 + k * step;
        ctx.strokeStyle = color;
        ctx.shadowColor = color;
        ctx.shadowBlur = 18;
        ctx.lineWidth = width;
        ctx.beginPath(); ctx.arc(cx, cy, radius, a - step * 0.46, a + step * 0.46); ctx.stroke();
        const s = { key: key(fan.c, i), x: cx + Math.cos(a) * radius, y: cy + Math.sin(a) * radius, a, step, cx, cy, radius, width };
        spots.push(s);
        if (selection.has(s.key)) marks.push(s);
      }
      ctx.shadowBlur = 0;
    };

    fans.forEach((fan, n) => {
      const cx = slot * (n + 0.5), cy = h / 2;

      // square frame with mounting holes, like a 120 mm fan
      ctx.fillStyle = body;
      ctx.strokeStyle = edge;
      ctx.lineWidth = 1.5;
      const half = R * 1.06, corner = R * 0.22;
      ctx.beginPath(); ctx.roundRect(cx - half, cy - half, half * 2, half * 2, corner); ctx.fill(); ctx.stroke();
      ctx.fillStyle = raised;
      for (const [dx, dy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) {
        ctx.beginPath(); ctx.arc(cx + dx * half * 0.84, cy + dy * half * 0.84, R * 0.05, 0, Math.PI * 2); ctx.fill();
      }

      ring(fan, cx, cy, R * 0.86, R * 0.1, fan.inner, fan.outer);

      ctx.strokeStyle = edge;
      ctx.lineWidth = R * 0.05;
      for (let b = 0; b < 9; b++) {
        const a = (b / 9) * Math.PI * 2;
        ctx.beginPath();
        ctx.arc(cx + Math.cos(a) * R * 0.3, cy + Math.sin(a) * R * 0.3, R * 0.42, a - 0.2, a + 0.9);
        ctx.stroke();
      }

      ctx.fillStyle = body;
      ctx.beginPath(); ctx.arc(cx, cy, R * 0.34, 0, Math.PI * 2); ctx.fill();
      ring(fan, cx, cy, R * 0.26, R * 0.09, 0, fan.inner);
      ctx.fillStyle = raised;
      ctx.beginPath(); ctx.arc(cx, cy, R * 0.15, 0, Math.PI * 2); ctx.fill();
    });

    // selection outlines on top
    for (const s of marks) {
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(s.cx, s.cy, s.radius + s.width / 2 + 3, s.a - s.step * 0.46, s.a + s.step * 0.46);
      ctx.stroke();
      ctx.fillStyle = "#ffffff";
      ctx.beginPath(); ctx.arc(s.x, s.y, 2.5, 0, Math.PI * 2); ctx.fill();
    }

    if (drag && drag.moved) {
      ctx.fillStyle = "rgba(255,45,149,0.12)";
      ctx.strokeStyle = accent;
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 3]);
      const x = Math.min(drag.x0, drag.x1), y = Math.min(drag.y0, drag.y1);
      ctx.fillRect(x, y, Math.abs(drag.x1 - drag.x0), Math.abs(drag.y1 - drag.y0));
      ctx.strokeRect(x, y, Math.abs(drag.x1 - drag.x0), Math.abs(drag.y1 - drag.y0));
      ctx.setLineDash([]);
    }

    const labels = $("fan-labels");
    labels.hidden = false;
    if (labels.childElementCount !== fans.length) {
      labels.style.gridTemplateColumns = `repeat(${fans.length}, 1fr)`;
      labels.replaceChildren(...fans.map((_, i) => el("span", null, `Fan ${i + 1}`)));
    }
  }

  // The K65, key by key. Each key glows in the color the keyboard was sent.
  function drawKeyboard(canvas) {
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    $("fan-labels").hidden = true;

    const css = getComputedStyle(document.documentElement);
    const body = css.getPropertyValue("--surface").trim();
    const edge = css.getPropertyValue("--line").trim();
    const raised = css.getPropertyValue("--raised").trim();
    const muted = css.getPropertyValue("--muted").trim();
    const accent = css.getPropertyValue("--accent").trim();
    const font = css.getPropertyValue("--font-mono").trim();

    // 60 quarter-units across, 5 rows; keep keys square-ish and centered.
    const pad = 14;
    const u = Math.min((w - pad * 2) / 60, (h - pad * 2) / 20);
    const ox = (w - u * 60) / 2, oy = (h - u * 20) / 2;
    const gap = Math.max(2, u * 0.35);

    // the keyboard case
    ctx.fillStyle = body;
    ctx.strokeStyle = edge;
    ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.roundRect(ox - pad * 0.7, oy - pad * 0.7, u * 60 + pad * 1.4, u * 20 + pad * 1.4, 12); ctx.fill(); ctx.stroke();

    spots = [];
    const marks = [];
    light.keys.forEach((k, i) => {
      const x = ox + k.x * u + gap / 2, y = oy + k.row * 4 * u + gap / 2;
      const kw = k.w * u - gap, kh = 4 * u - gap;
      const color = frame.keyboard[i] || "#000000";

      // glow under the key, then the keycap with a lit edge
      ctx.shadowColor = color;
      ctx.shadowBlur = 14;
      ctx.fillStyle = color;
      ctx.beginPath(); ctx.roundRect(x, y, kw, kh, 5); ctx.fill();
      ctx.shadowBlur = 0;
      ctx.fillStyle = raised;
      ctx.beginPath(); ctx.roundRect(x + 2, y + 2, kw - 4, kh - 6, 4); ctx.fill();
      ctx.fillStyle = color;
      ctx.globalAlpha = 0.28;
      ctx.beginPath(); ctx.roundRect(x + 2, y + 2, kw - 4, kh - 6, 4); ctx.fill();
      ctx.globalAlpha = 1;

      if (k.label) {
        ctx.fillStyle = muted;
        ctx.font = `${Math.max(9, u * 1.15)}px ${font}`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(k.label, x + kw / 2, y + kh / 2 - 2);
      }

      const s = { key: key(0, i), x: x + kw / 2, y: y + kh / 2, rect: [x, y, kw, kh] };
      spots.push(s);
      if (selection.has(s.key)) marks.push(s);
    });

    for (const s of marks) {
      const [x, y, kw, kh] = s.rect;
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.beginPath(); ctx.roundRect(x - 2, y - 2, kw + 4, kh + 4, 6); ctx.stroke();
    }

    if (drag && drag.moved) {
      ctx.fillStyle = "rgba(255,45,149,0.12)";
      ctx.strokeStyle = accent;
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 3]);
      const x = Math.min(drag.x0, drag.x1), y = Math.min(drag.y0, drag.y1);
      ctx.fillRect(x, y, Math.abs(drag.x1 - drag.x0), Math.abs(drag.y1 - drag.y0));
      ctx.strokeRect(x, y, Math.abs(drag.x1 - drag.x0), Math.abs(drag.y1 - drag.y0));
      ctx.setLineDash([]);
    }
  }

  /* ---------- layer editor ---------- */

  function syncEditor() {
    const l = layer();
    $("no-layer").hidden = !!l;
    $("layer-editor").hidden = !l;
    if (!l) return;
    const fx = EFFECTS[l.effect];
    const target = describeTarget(l.target);

    const name = $("layer-name");
    if (document.activeElement !== name) name.value = l.name;
    $("editing-target").textContent = `Layer · lit on ${l.target.kind === "all" ? target.toLowerCase() : target}`;

    const t = $("effect-thumb");
    t.className = `effect-thumb fx-thumb fx-${l.effect}`;
    t.style.background = thumbCss(l.effect, l.color1, l.color2);
    t.style.setProperty("--off", l.color2);
    $("effect-name").textContent = fx.name;
    $("effect-desc").textContent = fx.desc;

    $("target-desc").textContent = target;
    $("target-all").disabled = l.target.kind === "all";
    $("target-all").textContent = describeTarget({ kind: "all" });

    document.querySelectorAll("[data-spread]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.spread === l.spread)));
    $("spread-across").textContent = dev === "keyboard" ? "Left to right" : "Across fans";
    $("spread-per-fan").textContent = dev === "keyboard" ? "Top to bottom" : "Per fan";
    $("field-colors").hidden = !fx.c1;
    $("field-spread").hidden = !fx.spread;
    $("field-speed").hidden = !fx.speed;
    $("slot-2").hidden = !fx.c2;
    $("slot-1-name").textContent = fx.c1 || "";
    $("slot-2-name").textContent = fx.c2 || "";
    if (!fx.c2) activeSlot = "color1";
    for (const k of ["color1", "color2"]) {
      $(`${k}-chip`).style.setProperty("--chip", l[k]);
      $(`${k}-hex`).textContent = l[k];
    }
    $("slot-1").classList.toggle("active", activeSlot === "color1");
    $("slot-2").classList.toggle("active", activeSlot === "color2");
    $("slot-1").setAttribute("aria-selected", String(activeSlot === "color1"));
    $("slot-2").setAttribute("aria-selected", String(activeSlot === "color2"));
    if (fx.c1) syncPicker();

    $("brightness").value = l.brightness;
    $("brightness").style.setProperty("--fill", `${l.brightness}%`);
    $("brightness-val").textContent = `${l.brightness}%`;
    $("speed").value = l.speed;
    $("speed").style.setProperty("--fill", `${((l.speed - 1) / 9) * 100}%`);
    $("speed-val").textContent = l.speed;
  }

  /* ---------- color picker ---------- */

  function hexToHsv(hex) {
    const n = parseInt(hex.slice(1), 16);
    const r = (n >> 16) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
    const max = Math.max(r, g, b), d = max - Math.min(r, g, b);
    let h = 0;
    if (d) {
      if (max === r) h = ((g - b) / d) % 6;
      else if (max === g) h = (b - r) / d + 2;
      else h = (r - g) / d + 4;
      h = (h * 60 + 360) % 360;
    }
    return { h, s: max ? d / max : 0, v: max };
  }

  function hsvToHex({ h, s, v }) {
    const f = (k) => {
      const x = (k + h / 60) % 6;
      return Math.round((v - v * s * Math.max(0, Math.min(x, 4 - x, 1))) * 255);
    };
    return "#" + [f(5), f(3), f(1)].map((x) => x.toString(16).padStart(2, "0")).join("");
  }

  function syncPicker() {
    const hex = layer()[activeSlot];
    if (!hsv || hsvToHex(hsv) !== hex) hsv = hexToHsv(hex);
    $("sv").style.setProperty("--hue-color", hsvToHex({ h: hsv.h, s: 1, v: 1 }));
    const knob = $("sv-knob");
    knob.style.left = `${hsv.s * 100}%`;
    knob.style.top = `${(1 - hsv.v) * 100}%`;
    knob.style.setProperty("--knob", hex);
    const hk = $("hue-knob");
    hk.style.left = `${(hsv.h / 360) * 100}%`;
    hk.style.setProperty("--knob", hsvToHex({ h: hsv.h, s: 1, v: 1 }));
    $("hue").setAttribute("aria-valuenow", String(Math.round(hsv.h)));
    const input = $("hex-input");
    if (document.activeElement !== input) {
      input.value = hex.toUpperCase();
      input.classList.remove("bad");
    }
  }

  function pickHsv(next) {
    hsv = { ...hsv, ...next };
    editLayer({ [activeSlot]: hsvToHex(hsv) });
  }

  // Drag on an element; `move` gets the pointer position as 0..1 fractions.
  function draggable(elem, move) {
    const at = (e) => {
      const r = elem.getBoundingClientRect();
      move(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)));
    };
    elem.addEventListener("pointerdown", (e) => { e.preventDefault(); elem.setPointerCapture(e.pointerId); at(e); });
    elem.addEventListener("pointermove", (e) => { if (elem.hasPointerCapture(e.pointerId)) at(e); });
  }

  function initPicker() {
    draggable($("sv"), (x, y) => pickHsv({ s: x, v: 1 - y }));
    draggable($("hue"), (x) => pickHsv({ h: Math.min(x * 360, 359.9) }));

    const nudge = (e, keys) => {
      const step = e.shiftKey ? 0.1 : 0.02;
      const k = keys[e.key];
      if (!k) return;
      e.preventDefault();
      k(step);
    };
    $("sv").addEventListener("keydown", (e) => nudge(e, {
      ArrowLeft: (s) => pickHsv({ s: Math.max(0, hsv.s - s) }),
      ArrowRight: (s) => pickHsv({ s: Math.min(1, hsv.s + s) }),
      ArrowUp: (s) => pickHsv({ v: Math.min(1, hsv.v + s) }),
      ArrowDown: (s) => pickHsv({ v: Math.max(0, hsv.v - s) }),
    }));
    $("hue").addEventListener("keydown", (e) => nudge(e, {
      ArrowLeft: (s) => pickHsv({ h: (hsv.h - s * 360 + 360) % 360 }),
      ArrowRight: (s) => pickHsv({ h: (hsv.h + s * 360) % 360 }),
    }));

    const input = $("hex-input");
    input.addEventListener("input", () => {
      let v = input.value.trim();
      if (!v.startsWith("#")) v = "#" + v;
      const ok = /^#[0-9a-fA-F]{6}$/.test(v);
      input.classList.toggle("bad", !ok && v.length === 7);
      if (ok) {
        hsv = null;
        editLayer({ [activeSlot]: v.toLowerCase() });
      }
    });
    input.addEventListener("blur", () => layer() && syncPicker());
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") input.blur(); });
  }

  /* ---------- wiring ---------- */

  function renderAll() {
    if (!light) return;
    $("node-lighting").classList.toggle("keyboard", dev === "keyboard");
    $("select-label").textContent = dev === "keyboard" ? "Select keys" : "Select LEDs";
    $("select-hint").textContent = dev === "keyboard"
      ? "Click keys on the keyboard, or drag a box around them. Hold Shift to add more."
      : "Click LEDs in the preview, or drag a box around them. Hold Shift to add more.";
    renderProfiles();
    $("master").value = light.store.brightness;
    $("master").style.setProperty("--fill", `${light.store.brightness}%`);
    $("master-val").textContent = `${light.store.brightness}%`;
    renderLayers();
    renderSelection();
    syncEditor();
  }

  function swatch(name, hex, onPick) {
    const b = el("button", "preset");
    b.style.background = hex;
    b.title = name;
    b.setAttribute("aria-label", name);
    b.addEventListener("click", onPick);
    return b;
  }

  function initProfileMenu() {
    $("profile-btn").addEventListener("click", (e) => {
      e.stopPropagation();
      $("profile-menu").hidden ? openMenu() : closeMenu();
    });
    $("profile-menu").addEventListener("click", (e) => e.stopPropagation());
    document.addEventListener("click", closeMenu);
    document.addEventListener("keydown", (e) => {
      if (e.key !== "Escape") return;
      closeMenu();
      closePicker();
    });

    // New and duplicated profiles open straight into renaming, name selected.
    $("profile-new").addEventListener("click", () => {
      renaming = true;
      addProfile({
        name: `Profile ${light.store.profiles.length + 1}`,
        layers: [newLayer("gradient", { name: "Ember fade" })],
        keyboard: [newLayer("gradient", { name: "Ember fade" })],
      });
    });
    $("profile-dup").addEventListener("click", () => {
      const copy = structuredClone(profile());
      copy.name = `${copy.name} copy`.slice(0, 40);
      renaming = true;
      addProfile(copy);
    });
    $("profile-rename").addEventListener("click", () => { renaming = true; renderProfiles(); });
    $("profile-delete").addEventListener("click", () => {
      if (light.store.profiles.length <= 1) return;
      if (!deleteArmed) {
        deleteArmed = setTimeout(() => { deleteArmed = null; renderProfiles(); }, 3000);
        renderProfiles();
        return;
      }
      clearTimeout(deleteArmed);
      deleteArmed = null;
      light.store.profiles.splice(light.store.active, 1);
      light.store.active = Math.min(light.store.active, light.store.profiles.length - 1);
      layerIndex = Math.max(0, stack().length - 1);
      hsv = null;
      closeMenu();
      changed();
    });
  }

  async function init() {
    const state = await invoke("get_lighting");
    light = { store: state.store, layout: state.layout, keys: state.keys };
    layerIndex = Math.max(0, stack().length - 1);

    $("quick-colors").replaceChildren(...QUICK_COLORS.map(([n, hex]) => swatch(n, hex, () => { hsv = null; editLayer({ [activeSlot]: hex }); })));
    $("paint-swatches").replaceChildren(...QUICK_COLORS.map(([n, hex]) => swatch(`Paint ${n.toLowerCase()}`, hex, () => paint(hex))));

    document.querySelectorAll("[data-spread]").forEach((b) => b.addEventListener("click", () => editLayer({ spread: b.dataset.spread })));
    document.querySelectorAll("[data-slot]").forEach((b) => b.addEventListener("click", () => {
      activeSlot = b.dataset.slot;
      hsv = null;
      syncEditor();
    }));
    $("brightness").addEventListener("input", (e) => editLayer({ brightness: Number(e.target.value) }));
    $("speed").addEventListener("input", (e) => editLayer({ speed: Number(e.target.value) }));
    $("master").addEventListener("input", (e) => { light.store.brightness = Number(e.target.value); changed(); });

    $("layer-name").addEventListener("input", (e) => {
      layer().name = e.target.value;
      renderLayers();
      send();
    });
    $("layer-name").addEventListener("blur", (e) => { if (!e.target.value.trim()) editLayer({ name: EFFECTS[layer().effect].name }); });
    $("layer-name").addEventListener("keydown", (e) => { if (e.key === "Enter") e.target.blur(); });
    $("target-sel").addEventListener("click", () => editLayer({ target: ledsTarget([...selection]) }));
    $("target-all").addEventListener("click", () => editLayer({ target: { kind: "all" } }));
    $("layer-add").addEventListener("click", () => openPicker("add"));
    $("no-layer-add").addEventListener("click", () => openPicker("add"));
    $("effect-change").addEventListener("click", () => openPicker("change"));
    $("fx-close").addEventListener("click", closePicker);
    $("fx-overlay").addEventListener("click", (e) => { if (e.target === $("fx-overlay")) closePicker(); });

    initProfileMenu();
    initPicker();
    initCanvas();
    renderAll();

    const { listen } = window.__TAURI__.event;
    await listen("lighting-frame", (e) => { frame = e.payload; drawFans(); });
    window.addEventListener("resize", drawFans);
  }

  // Show the lighting screen for "node" (fans) or "keyboard".
  function show(which) {
    if (which !== dev) {
      dev = which;
      selection.clear();
      layerIndex = Math.max(0, stack().length - 1);
      activeSlot = "color1";
      hsv = null;
    }
    renderAll();
  }

  return { init, show };
})();
