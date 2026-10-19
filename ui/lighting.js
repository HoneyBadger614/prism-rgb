// Prism lighting screen for the Lighting Node Pro, K65 keyboard and Nightsword mouse:
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
  // (c1/c2 are color labels; missing = not used; `more` names the added
  // colors and numbers them from `from`; `direction` = can run the other
  // way), and its starting settings. A `bg` effect's color 2 is a
  // background. `typing` effects start on every LED.
  const GROUPS = ["Colors", "Moving", "Typing", "Your computer"];
  const EFFECTS = {
    static: { group: "Colors", name: "Static", desc: "One solid color", c1: "Color", start: { color1: PURPLE } },
    gradient: { group: "Colors", name: "Gradient", desc: "Colors fading into each other", c1: "Color 1", c2: "Color 2", more: { name: "Color", from: 3 }, spread: true, start: { color1: PURPLE, color2: PINK } },
    wave: { group: "Colors", name: "Wave", desc: "A gradient that keeps flowing", c1: "Color 1", c2: "Color 2", more: { name: "Color", from: 3 }, spread: true, direction: true, speed: true, start: { color1: PURPLE, color2: PINK } },
    breathe: { group: "Colors", name: "Breathe", desc: "Slowly fades in and out, a color each breath", c1: "Color 1", c2: "Color 2", more: { name: "Color", from: 3 }, speed: true, start: { color1: PURPLE, color2: PINK, speed: 3 } },
    rainbow: { group: "Colors", name: "Rainbow", desc: "Every color, flowing", spread: true, direction: true, speed: true, start: { speed: 4 } },
    "color-cycle": { group: "Colors", name: "Color cycle", desc: "Everything drifts through every color together", speed: true, start: { speed: 4 } },
    comet: { group: "Moving", bg: true, name: "Comet", desc: "A bright head with a fading tail", c1: "Head 1", c2: "Background", more: { name: "Head", from: 2 }, spread: true, direction: true, speed: true, start: { color1: PINK, color2: "#140624", spread: "per-fan" } },
    strobe: { group: "Moving", bg: true, name: "Strobe", desc: "Fast flashing", c1: "Flash 1", c2: "Between", more: { name: "Flash", from: 2 }, speed: true, start: { color1: WHITE, color2: "#000000", speed: 4 } },
    "key-glow": { group: "Typing", typing: true, name: "Key glow", desc: "Each key you press lights up, then fades. Only that key", c1: "Glow 1", more: { name: "Glow", from: 2 }, random: true, speed: true, start: { color1: PINK, speed: 5, palette: [], random: false } },
    ripple: { group: "Typing", typing: true, name: "Typing ripple", desc: "Rings spread from each key you press, over your other layers. Add it to each device you want them on", c1: "Ripple 1", more: { name: "Ripple", from: 2 }, random: true, speed: true, start: { color1: PINK, speed: 5, palette: [], random: false } },
    plasma: { group: "Colors", name: "Plasma", desc: "Soft blobs of color drifting and blending", c1: "Color 1", c2: "Color 2", more: { name: "Color", from: 3 }, speed: true, start: { color1: PURPLE, color2: "#22d3ee", palette: [PINK], speed: 4 } },
    "color-split": { group: "Colors", name: "Color split", desc: "Each fan (or zone) its own color, passed along every few seconds", c1: "Color 1", c2: "Color 2", more: { name: "Color", from: 3 }, spread: true, direction: true, speed: true, start: { color1: PURPLE, color2: PINK, palette: ["#22d3ee"], speed: 4 } },
    spin: { group: "Moving", bg: true, name: "Spin", desc: "Blades of color turning round each fan, one per color", c1: "Blade 1", c2: "Background", more: { name: "Blade", from: 2 }, direction: true, speed: true, start: { color1: PINK, color2: "#140624", palette: ["#22d3ee"] } },
    marquee: { group: "Moving", name: "Marquee", desc: "Blocks of color stepping round, like theatre lights", c1: "Color 1", c2: "Color 2", more: { name: "Color", from: 3 }, spread: true, direction: true, speed: true, start: { color1: "#ffb000", color2: "#000000", spread: "per-fan", speed: 3 } },
    twinkle: { group: "Moving", bg: true, name: "Twinkle", desc: "Stars twinkling at random", c1: "Star 1", c2: "Sky", more: { name: "Star", from: 2 }, speed: true, start: { color1: WHITE, color2: "#0b0620", palette: ["#22d3ee"], speed: 4 } },
    fire: { group: "Moving", name: "Fire", desc: "Flickering flames, hotter further down", c1: "Embers", c2: "Flames", more: { name: "Hot", from: 1 }, speed: true, start: { color1: "#5a0000", color2: "#ff4500", palette: ["#ffc400"], speed: 5 } },
    rain: { group: "Moving", bg: true, name: "Rain", desc: "Drops running down. Best on the keyboard", c1: "Drop 1", c2: "Background", more: { name: "Drop", from: 2 }, direction: true, speed: true, start: { color1: "#22e55b", color2: "#000000", speed: 4 } },
    scanner: { group: "Moving", bg: true, name: "Scanner", desc: "One light sweeping back and forth", c1: "Light 1", c2: "Background", more: { name: "Light", from: 2 }, spread: true, direction: true, speed: true, start: { color1: "#ff1a1a", color2: "#000000", speed: 5 } },
    heartbeat: { group: "Moving", bg: true, name: "Heartbeat", desc: "A double pulse, then a rest", c1: "Beat 1", c2: "Between", more: { name: "Beat", from: 2 }, speed: true, start: { color1: "#ff1a1a", color2: "#000000", speed: 5 } },
    heatmap: { group: "Typing", typing: true, name: "Typing heatmap", desc: "Keys you press most glow warmer, then cool down", c1: "Cool", c2: "Warm", more: { name: "Hot", from: 1 }, speed: true, start: { color1: "#2d5bff", color2: "#ff7a00", palette: ["#ff1a1a"], speed: 5 } },
    reactive: { group: "Typing", typing: true, name: "Reactive keys", desc: "Keys you press stay lit for a while, then fade", c1: "Key 1", more: { name: "Key", from: 2 }, random: true, speed: true, start: { color1: "#22d3ee", speed: 5, palette: [], random: false } },
    "typing-speed": { group: "Typing", name: "Typing speed", desc: "Everything warms up the faster you type", c1: "Resting", c2: "Typing", more: { name: "Fast", from: 1 }, speed: true, start: { color1: "#2d5bff", color2: PURPLE, palette: ["#ff1a1a"], speed: 5 } },
    "cpu-temp": { group: "Your computer", name: "CPU temperature", desc: "Cool to hot as the processor warms up (40–90 °C)", c1: "Cool", c2: "Warm", more: { name: "Hot", from: 1 }, start: { color1: "#2d5bff", color2: "#22e55b", palette: ["#ff7a00", "#ff1a1a"] } },
    "gpu-temp": { group: "Your computer", name: "GPU temperature", desc: "Cool to hot as the graphics card warms up (40–90 °C)", c1: "Cool", c2: "Warm", more: { name: "Hot", from: 1 }, start: { color1: "#2d5bff", color2: "#22e55b", palette: ["#ff7a00", "#ff1a1a"] } },
    music: { group: "Your computer", name: "Music", desc: "Follows what's playing, from the bass to the treble", c1: "Quiet", c2: "Loud", more: { name: "Louder", from: 1 }, bass: true, start: { color1: PURPLE, color2: PINK, palette: [WHITE] } },
  };

  // Music: where the bass goes on each device, as [label, spread, reverse].
  // The treble is at the other end.
  const BASS = {
    node: [["Bass on the first fan", "across", false], ["Bass on the last fan", "across", true], ["Each fan: bass at the start", "per-fan", false], ["Each fan: bass halfway round", "per-fan", true]],
    keyboard: [["Bass on the left", "across", false], ["Bass on the right", "across", true], ["Bass at the bottom", "per-fan", true], ["Bass at the top", "per-fan", false]],
    mouse: [["Bass at the front", "across", false], ["Bass at the back", "across", true]],
    gpu: [],
    board: [["Bass at the start", "across", false], ["Bass at the end", "across", true], ["Each strip: bass at the start", "per-fan", false], ["Each strip: bass at the end", "per-fan", true]],
  };

  const ICON = {
    eye: '<svg viewBox="0 0 24 24"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>',
    eyeOff: '<svg viewBox="0 0 24 24"><path d="M3 3l18 18M10.6 5.1A10.8 10.8 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4.2M6.6 6.6C3.9 8.4 2 12 2 12s3.5 7 10 7c1.7 0 3.2-.5 4.5-1.1"/></svg>',
    up: '<svg viewBox="0 0 24 24"><path d="M12 19V5M6 11l6-6 6 6"/></svg>',
    down: '<svg viewBox="0 0 24 24"><path d="M12 5v14M6 13l6 6 6-6"/></svg>',
    trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>',
    check: '<svg viewBox="0 0 24 24"><path d="M5 12l5 5 9-10"/></svg>',
  };

  let light = null;          // { store, layout (fans), keys (keyboard), zones (mouse) }
  let frame = { node: [], keyboard: [], mouse: [], gpu: [], board: [] }; // latest colors sent to each device
  let dev = "node";          // which device is showing: "node", "keyboard", "mouse", "gpu" or "board"
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
  const FIELD = { node: "layers", keyboard: "keyboard", mouse: "mouse", gpu: "gpu", board: "motherboard" };

  // Motherboard LEDs per channel: built-in lights, then each ARGB header.
  function boardLens() {
    const b = light.board;
    if (!b) return [];
    const set = light.store.argb_leds || [];
    return [b.fixed, ...Array.from({ length: b.argb_headers }, (_, h) => set[h] ?? b.default_argb_leds)];
  }
  const headerName = (h) => `ARGB header ${h + 1}`;
  const stack = () => profile()[FIELD[dev]];
  const layer = () => stack()[layerIndex];
  const key = (c, i) => `${c}:${i}`;
  const ref = (k) => k.split(":").map(Number);

  function allLeds() {
    if (dev === "keyboard") return light.keys.map((_, i) => key(0, i));
    if (dev === "mouse") return light.zones.map((_, i) => key(0, i));
    if (dev === "gpu") return [key(0, 0)];
    if (dev === "board") return boardLens().flatMap((n, c) => Array.from({ length: n }, (_, i) => key(c, i)));
    const out = [];
    light.layout.forEach((ch, c) => {
      const n = ch.pieces.reduce((t, p) => t + p.inner + p.outer, 0);
      for (let i = 0; i < n; i++) out.push(key(c, i));
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
    if (dev === "mouse") return [["All", allLeds()], ...light.zones.map((name, i) => [name, [key(0, i)]])];
    if (dev === "gpu") return [["All", allLeds()]];
    if (dev === "board") {
      const lens = boardLens();
      const strip = (c) => Array.from({ length: lens[c] }, (_, i) => key(c, i));
      return [["All", allLeds()], ["Board lights", strip(0)], ...lens.slice(1).map((_, h) => [headerName(h), strip(h + 1)])];
    }
    const g = [["All", allLeds()], ["Hubs", []], ["Rings", []]];
    for (const p of pieceList()) {
      const leds = [];
      for (let j = 0; j < p.leds; j++) {
        const k = key(p.c, p.base + j);
        leds.push(k);
        if (p.shape === "fan") (j < p.inner ? g[1][1] : g[2][1]).push(k);
      }
      g.push([p.label, leds]);
    }
    // Only fans with a center hub have hubs.
    return g.filter(([n, set]) => !(n === "Hubs" || n === "Rings") || g[1][1].length);
  }

  // Every fan and strip, in order: where its LEDs start and its label
  // ("Fan 2", "Strip 1").
  function pieceList() {
    const out = [];
    const counts = {};
    light.layout.forEach((ch, c) => {
      let base = 0;
      for (const p of ch.pieces) {
        const kind = p.shape === "strip" ? "Strip" : "Fan";
        counts[kind] = (counts[kind] || 0) + 1;
        out.push({ ...p, c, base, leds: p.inner + p.outer, label: `${kind} ${counts[kind]}` });
        base += p.inner + p.outer;
      }
    });
    return out;
  }

  const LEGACY = [["ll", "ll", "ll"], []]; // what Prism assumed before channels could be set
  const MAX_CHANNEL_LEDS = 204;

  // Rebuilds the layout from the settings (the screen keeps its own copy).
  function syncLayout() {
    const ids = light.store.node_channels || LEGACY;
    light.layout = ids.map((ch) => ({ pieces: ch.map((id) => light.pieces.find((p) => p.id === id)).filter(Boolean) }));
  }

  // "What's plugged in": the pieces on each channel, in wiring order.
  function renderChannels() {
    const bar = $("channel-bar");
    bar.hidden = dev !== "node";
    if (dev !== "node") return;
    const channels = light.node ? light.node[1] : 2;
    const ids = (light.store.node_channels || LEGACY).map((c) => [...c]);
    while (ids.length < channels) ids.push([]);
    const setIds = (next) => {
      light.store.node_channels = next;
      syncLayout();
      selection.clear();
      changed();
    };
    const rows = ids.slice(0, channels).map((ch, c) => {
      const row = el("div", "channel-row");
      const used = ch.reduce((t, id) => t + (light.pieces.find((p) => p.id === id)?.inner || 0) + (light.pieces.find((p) => p.id === id)?.outer || 0), 0);
      const head = el("div", "channel-head");
      head.append(el("span", "label", `Channel ${c + 1}`), el("span", "channel-count", `${used} / ${MAX_CHANNEL_LEDS} LEDs`));
      row.append(head);
      const chips = el("div", "channel-chips");
      ch.forEach((id, i) => {
        const p = light.pieces.find((x) => x.id === id);
        const chip = el("span", "channel-chip", p ? p.name : id);
        const x = el("button", "chip-x", "×");
        x.title = "Remove";
        x.setAttribute("aria-label", `Remove ${p ? p.name : id}`);
        x.addEventListener("click", () => { const next = ids.map((a) => [...a]); next[c].splice(i, 1); setIds(next); });
        chip.append(x);
        chips.append(chip);
      });
      if (!ch.length) chips.append(el("span", "hint", "Nothing plugged in"));
      const add = el("select", "channel-add");
      add.setAttribute("aria-label", `Add to channel ${c + 1}`);
      add.append(new Option("Add a fan or strip…", ""));
      for (const p of light.pieces) {
        const o = new Option(`${p.name} (${p.inner + p.outer} LEDs)`, p.id);
        o.disabled = used + p.inner + p.outer > MAX_CHANNEL_LEDS;
        add.append(o);
      }
      add.addEventListener("change", () => {
        if (!add.value) return;
        const next = ids.map((a) => [...a]);
        next[c].push(add.value);
        setIds(next);
      });
      row.append(chips, add);
      return row;
    });
    const note = light.node && !light.node[2]
      ? `Your ${light.node[0]} is supported from other projects' notes but hasn't been tested with Prism yet. If something looks wrong, please report it.`
      : "Add each fan or strip in the order it's wired, starting nearest the controller.";
    const grid = el("div", "channel-grid");
    grid.append(...rows);
    bar.replaceChildren(el("span", "label", "What's plugged in"), grid, el("p", "hint", note));
  }

  // A named group's LEDs; none if this device doesn't have it.
  const group = (name) => (groups().find(([n]) => n === name) || [null, []])[1];
  const ledsTarget = (keys) => ({ kind: "leds", leds: keys.map(ref) });
  const sameSet = (a, b) => a.length === b.length && a.every((k) => b.includes(k));

  function describeTarget(target) {
    if (target.kind === "all") return `All ${unit(2)}`;
    const leds = target.leds.map(([c, i]) => key(c, i));
    if (!leds.length) return "No LEDs";
    for (const [name, set] of groups()) if (sameSet(leds, set)) return name === "All" ? describeTarget({ kind: "all" }) : name;
    return `${leds.length} ${unit(leds.length)}`;
  }

  const UNITS = { node: ["LED", "LEDs"], keyboard: ["key", "keys"], mouse: ["zone", "zones"], gpu: ["light", "lights"], board: ["LED", "LEDs"] };
  const unit = (n) => UNITS[dev][n === 1 ? 0 : 1];

  function send() {
    // Coalesce rapid changes (slider drags) into one update per screen refresh.
    if (sendQueued) return;
    sendQueued = true;
    requestAnimationFrame(async () => {
      sendQueued = false;
      await invoke("set_lighting", { store: light.store });
    });
  }

  const listeners = [];

  function changed() {
    renderAll();
    send();
    listeners.forEach((fn) => fn());
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

  // Ready-made multi-layer looks for the mouse.
  const MOUSE_COMBOS = [
    {
      name: "Ember split", desc: "Purple logo, hot pink front and scroll wheel",
      thumb: `linear-gradient(90deg, ${PINK} 0 50%, ${PURPLE} 50%)`,
      layers: () => [
        newLayer("static", { name: "Logo", color1: PURPLE }),
        newLayer("static", { name: "Front & wheel", color1: PINK, target: ledsTarget([...group("Front"), ...group("Scroll wheel")]) }),
      ],
    },
    {
      name: "Logo only", desc: "Just the logo glows; everything else off",
      thumb: `radial-gradient(circle, ${PINK} 0 26%, #000 28%)`,
      layers: () => [
        newLayer("static", { name: "Off", color1: "#000000" }),
        newLayer("breathe", { name: "Logo", color1: PINK, color2: PURPLE, target: ledsTarget(group("Logo")) }),
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
  function thumbCss(effect, c1, c2, more = []) {
    const all = [c1, c2, ...more].join(", ");
    switch (effect) {
      case "static": return c1;
      case "gradient": case "breathe": return `linear-gradient(90deg, ${all})`;
      case "wave": return `linear-gradient(90deg, ${all}, ${c1})`;
      case "rainbow": case "color-cycle": return "linear-gradient(90deg,#f00,#ff0,#0f0,#0ff,#00f,#f0f,#f00)";
      case "comet": return `linear-gradient(90deg, ${c2} 0 45%, ${c1} 70%, ${c2} 72%)`;
      case "strobe": return c1;
      case "key-glow": return `radial-gradient(circle at 50% 50%, ${c1} 0 18%, #140624 40%)`;
      case "ripple": { const bg = "#140624"; return `radial-gradient(circle, ${bg} 0 18%, ${c1} 22% 30%, ${bg} 34% 48%, ${c1} 52% 58%, ${bg} 62%)`; }
      case "plasma": return `radial-gradient(circle at 25% 35%, ${c1}, transparent 55%), radial-gradient(circle at 75% 65%, ${c2}, transparent 55%), ${more[0] || c1}`;
      case "color-split": return `linear-gradient(90deg, ${blocks([c1, c2, ...more])})`;
      case "spin": {
        const blades = more.length ? [c1, ...more] : [c1, c1];
        const at = (i, f) => `${((i + f) * 100) / blades.length}%`;
        return `conic-gradient(${blades.map((c, i) => `${c2} ${at(i, 0)}, ${c} ${at(i, 0.15)} ${at(i, 0.45)}, ${c2} ${at(i, 0.45)} ${at(i, 1)}`).join(", ")})`;
      }
      case "marquee": return `repeating-linear-gradient(90deg, ${[c1, c2, ...more].map((c, i) => `${c} ${i * 12}px ${(i + 1) * 12}px`).join(", ")})`;
      case "twinkle": return [c1, more[0] || c1, c1, more[1] || c1].map((c, i) => `radial-gradient(circle at ${[18, 64, 40, 84][i]}% ${[30, 24, 72, 66][i]}%, ${c} 0 2px, transparent 4px)`).join(", ") + `, ${c2}`;
      case "fire": return `linear-gradient(0deg, ${[c1, c2, ...more].reverse().join(", ")})`;
      case "rain": return `repeating-linear-gradient(90deg, ${c2} 0 9px, transparent 9px 12px), repeating-linear-gradient(180deg, ${c2} 0 14px, ${c1} 14px 22px)`;
      case "scanner": return `linear-gradient(90deg, ${c2} 0 42%, ${c1} 50%, ${c2} 58%)`;
      case "heartbeat": return `radial-gradient(circle, ${c1} 0 22%, ${c2} 60%)`;
      case "heatmap": return `radial-gradient(circle at 40% 55%, ${more.at(-1) || c2} 0 10%, ${c2} 24%, ${c1} 40%, #140624 62%)`;
      case "reactive": return `radial-gradient(circle at 30% 50%, ${c1} 0 12%, transparent 16%), radial-gradient(circle at 62% 50%, ${more[0] || c1} 0 12%, transparent 16%), #140624`;
      case "typing-speed": case "cpu-temp": case "gpu-temp": return `linear-gradient(90deg, ${all})`;
      case "music": return `repeating-linear-gradient(90deg, #140624 0 3px, transparent 3px 11px), linear-gradient(0deg, ${all} 70%, #140624 70%)`;
    }
    return c1;
  }

  // Equal hard-edged blocks of each color, for a CSS gradient.
  function blocks(colors) {
    return colors.map((c, i) => `${c} ${(i * 100) / colors.length}% ${((i + 1) * 100) / colors.length}%`).join(", ");
  }

  function thumb(effect, c1, c2, more) {
    const t = el("span", `fx-thumb fx-${effect}`);
    t.style.background = thumbCss(effect, c1, c2, more);
    if (effect === "strobe") t.style.setProperty("--off", c2);
    if (effect === "marquee") t.style.setProperty("--period", `${(2 + (more || []).length) * 12}px`);
    return t;
  }

  function openPicker(mode) {
    pickerMode = mode;
    const l = layer();
    $("fx-title").textContent = mode === "add" ? "Add effect" : "Change effect";
    $("fx-sub").textContent = mode === "add"
      ? `Adds a new layer on top, lit on ${selection.size ? `your ${selection.size} selected ${unit(selection.size)} (typing effects: all ${unit(2)})` : `all ${unit(2)}`}.`
      : `For the layer “${l.name}”. Its colors and LEDs stay the same.`;
    const combos = { node: COMBOS, keyboard: KEY_COMBOS, mouse: MOUSE_COMBOS }[dev] || [];
    $("fx-combos-wrap").hidden = mode !== "add" || !combos.length;

    const cards = Object.entries(EFFECTS).map(([k, e]) => {
      const card = el("button", "fx-card" + (mode === "change" && l.effect === k ? " current" : ""));
      const c1 = mode === "change" && e.c1 ? l.color1 : e.start.color1 || PURPLE;
      const c2 = mode === "change" && e.c2 ? l.color2 : e.start.color2 || PINK;
      const more = mode === "change" && e.more ? l.palette || [] : [];
      card.append(thumb(k, c1, c2, more), el("span", "fx-name", e.name), el("span", "fx-desc", e.desc));
      card.addEventListener("click", () => {
        closePicker();
        if (pickerMode === "change") {
          const keep = { effect: k, name: l.name === EFFECTS[l.effect].name ? e.name : l.name };
          // The colors stay. Only a move to a background effect swaps
          // color 2 for a dark background, so it doesn't light everything.
          if (e.bg && !EFFECTS[l.effect].bg) keep.color2 = e.start.color2;
          if (!e.c2 && activeSlot === "color2") activeSlot = "color1";
          hsv = null;
          editLayer(keep);
        } else {
          // Effects that react to typing start on everything; others on the
          // selection, if there is one.
          const target = selection.size && !e.typing ? ledsTarget([...selection]) : { kind: "all" };
          pushLayers([newLayer(k, { target })]);
        }
      });
      return [e.group, card];
    });
    $("fx-effects").replaceChildren(...GROUPS.flatMap((g) => [el("h3", "fx-group", g), ...cards.filter(([cg]) => cg === g).map(([, c]) => c)]));

    $("fx-combos").replaceChildren(...combos.map((c) => {
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

      const sw = thumb(l.effect, l.color1, l.color2, fx.more ? l.palette || [] : []);
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
    if (dev !== "node") {
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
        // A zone with an area counts if the box touches it; an LED if the
        // box covers its center.
        const touches = (sp) => sp.rect
          ? sp.rect[0] <= xb && sp.rect[0] + sp.rect[2] >= xa && sp.rect[1] <= yb && sp.rect[1] + sp.rect[3] >= ya
          : sp.x >= xa && sp.x <= xb && sp.y >= ya && sp.y <= yb;
        for (const sp of spots) if (touches(sp)) selection.add(sp.key);
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

  // The dashed selection box while dragging, on any preview.
  function drawDragBox(ctx) {
    if (!drag || !drag.moved) return;
    const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
    ctx.save();
    ctx.setTransform(window.devicePixelRatio || 1, 0, 0, window.devicePixelRatio || 1, 0, 0);
    ctx.fillStyle = "rgba(255,45,149,0.12)";
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 3]);
    const x = Math.min(drag.x0, drag.x1), y = Math.min(drag.y0, drag.y1);
    ctx.fillRect(x, y, Math.abs(drag.x1 - drag.x0), Math.abs(drag.y1 - drag.y0));
    ctx.strokeRect(x, y, Math.abs(drag.x1 - drag.x0), Math.abs(drag.y1 - drag.y0));
    ctx.restore();
  }

  function drawFans() {
    const canvas = $("fans");
    if (!light || canvas.offsetParent === null) return;
    if (dev === "keyboard") return drawKeyboard(canvas);
    if (dev === "mouse") return drawMouse(canvas);
    if (dev === "gpu") return drawGpu(canvas);
    if (dev === "board") return drawBoard(canvas);
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const fans = pieceList();
    spots = [];
    if (!fans.length) {
      ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--muted").trim();
      ctx.font = "13px sans-serif";
      ctx.textAlign = "center";
      ctx.fillText("Nothing set up yet: add what's plugged in below.", w / 2, h / 2);
      $("fan-labels").hidden = true;
      return;
    }

    const css = getComputedStyle(document.documentElement);
    const body = css.getPropertyValue("--surface").trim();
    const edge = css.getPropertyValue("--line").trim();
    const raised = css.getPropertyValue("--raised").trim();
    const accent = css.getPropertyValue("--accent").trim();
    const slot = w / fans.length;
    const R = Math.min(slot, h) * 0.44;
    const marks = [];
    const color = (fan, i) => (frame.node[fan.c] || [])[fan.base + i] || "#000000";

    // A ring of glowing light segments; remembers where each LED is.
    const ring = (fan, cx, cy, radius, width, first, count) => {
      const step = (Math.PI * 2) / count;
      for (let k = 0; k < count; k++) {
        const i = first + k;
        const c = color(fan, i);
        const a = -Math.PI / 2 + k * step;
        ctx.strokeStyle = c;
        ctx.shadowColor = c;
        ctx.shadowBlur = 18;
        ctx.lineWidth = width;
        ctx.beginPath(); ctx.arc(cx, cy, radius, a - step * 0.46, a + step * 0.46); ctx.stroke();
        const s = { key: key(fan.c, fan.base + i), x: cx + Math.cos(a) * radius, y: cy + Math.sin(a) * radius, a, step, cx, cy, radius, width };
        spots.push(s);
        if (selection.has(s.key)) marks.push(s);
      }
      ctx.shadowBlur = 0;
    };

    fans.forEach((fan, n) => {
      const cx = slot * (n + 0.5), cy = h / 2;

      if (fan.shape === "strip") {
        // a bar of LEDs, top to bottom
        const len = Math.min(h - 16, R * 2.2);
        const step = len / fan.leds;
        const r = Math.max(1.5, Math.min(7, step * 0.38));
        ctx.fillStyle = body;
        ctx.strokeStyle = edge;
        ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.roundRect(cx - r * 2.2, cy - len / 2 - 6, r * 4.4, len + 12, r * 2); ctx.fill(); ctx.stroke();
        for (let i = 0; i < fan.leds; i++) {
          const y = cy - len / 2 + step * (i + 0.5);
          const c = color(fan, i);
          ctx.shadowColor = c;
          ctx.shadowBlur = 14;
          ctx.fillStyle = c;
          ctx.beginPath(); ctx.arc(cx, y, r, 0, Math.PI * 2); ctx.fill();
          ctx.shadowBlur = 0;
          const s = { key: key(fan.c, fan.base + i), x: cx, y, dot: r };
          spots.push(s);
          if (selection.has(s.key)) marks.push(s);
        }
        return;
      }

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

      if (fan.outer) ring(fan, cx, cy, R * 0.86, R * 0.1, fan.inner, fan.outer);

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
      if (fan.inner) ring(fan, cx, cy, R * 0.26, R * 0.09, 0, fan.inner);
      ctx.fillStyle = raised;
      ctx.beginPath(); ctx.arc(cx, cy, R * 0.15, 0, Math.PI * 2); ctx.fill();
    });

    // selection outlines on top
    for (const s of marks) {
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.beginPath();
      if (s.dot) ctx.arc(s.x, s.y, s.dot + 3, 0, Math.PI * 2);
      else ctx.arc(s.cx, s.cy, s.radius + s.width / 2 + 3, s.a - s.step * 0.46, s.a + s.step * 0.46);
      ctx.stroke();
      if (!s.dot) {
        ctx.fillStyle = "#ffffff";
        ctx.beginPath(); ctx.arc(s.x, s.y, 2.5, 0, Math.PI * 2); ctx.fill();
      }
    }

    drawDragBox(ctx);

    const labels = $("fan-labels");
    labels.hidden = false;
    const names = fans.map((f) => f.label).join("|");
    if (labels.dataset.names !== names) {
      labels.dataset.names = names;
      labels.style.gridTemplateColumns = `repeat(${fans.length}, 1fr)`;
      labels.replaceChildren(...fans.map((f) => el("span", null, f.label)));
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

      if (k.label) drawKeyLabel(ctx, k.label, x, y, kw, kh, u, muted, font);

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

    drawDragBox(ctx);
  }

  // The Nightsword from above, with its four zones glowing and labeled.
  // Drawn in a 120 x 190 box (front at the top), scaled to fit.
  const MOUSE_BODY = "M60 6c30 0 50 22 50 60v58c0 36-22 60-50 60S10 160 10 124V66C10 28 30 6 60 6z";
  // Each zone, in ZONES order: how to draw it, and a box around it for clicking.
  // Where each kind of zone is drawn, by name (each mouse has some of them).
  const MOUSE_SHAPES = {
    "Front": { path: "M36 20c10-7 38-7 48 0", width: 5, box: [34, 12, 52, 12], side: 1 },   // arc along the front
    "Scroll wheel": { rect: [55, 26, 10, 24, 5], box: [53, 24, 14, 28], side: 1 },
    "Logo": { path: "M42 150c6 10 30 10 36 0", width: 6, box: [40, 146, 40, 16], side: -1 }, // on the palm
    "DPI indicator": { rect: [12, 80, 5, 40, 2.5], box: [9, 78, 11, 44], side: -1 },        // left side
    "Back": { path: "M38 172c9 8 35 8 44 0", width: 4, box: [36, 167, 48, 14], side: 1 },   // under the logo
    "Thumb": { rect: [14, 70, 6, 50, 3], box: [11, 68, 12, 54], side: -1 },                 // thumb side
    "Side lights": { rect: [103, 80, 5, 40, 2.5], box: [100, 78, 11, 44], side: 1 },        // right side
  };
  const mouseZones = () => light.zones.map((n) => MOUSE_SHAPES[n] || MOUSE_SHAPES.Back);

  function drawMouse(canvas) {
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    $("fan-labels").hidden = true;

    const css = getComputedStyle(document.documentElement);
    const raised = css.getPropertyValue("--raised").trim();
    const edge = css.getPropertyValue("--line").trim();
    const muted = css.getPropertyValue("--muted").trim();
    const font = css.getPropertyValue("--font-mono").trim();

    const s = Math.min((h - 20) / 190, (w * 0.42) / 120);
    const ox = w / 2 - 60 * s, oy = (h - 190 * s) / 2;
    const toScreen = (x, y) => [ox + x * s, oy + y * s];
    const inMouse = (fn) => {
      ctx.save();
      ctx.setTransform(dpr * s, 0, 0, dpr * s, dpr * ox, dpr * oy);
      fn();
      ctx.restore();
    };

    // body and button split
    inMouse(() => {
      ctx.fillStyle = raised;
      ctx.strokeStyle = edge;
      ctx.lineWidth = 2 / s * 0.9;
      const body = new Path2D(MOUSE_BODY);
      ctx.fill(body);
      ctx.stroke(body);
      ctx.beginPath(); ctx.moveTo(60, 6); ctx.lineTo(60, 64); ctx.stroke();
    });

    spots = [];
    const marks = [];
    mouseZones().forEach((z, i) => {
      const c = frame.mouse[i] || "#000000";
      inMouse(() => {
        ctx.shadowColor = c;
        ctx.shadowBlur = 18 * s;
        if (z.rect) {
          const [x, y, rw, rh, r] = z.rect;
          ctx.fillStyle = c;
          ctx.beginPath(); ctx.roundRect(x, y, rw, rh, r); ctx.fill();
        } else {
          ctx.strokeStyle = c;
          ctx.lineWidth = z.width;
          ctx.lineCap = "round";
          ctx.stroke(new Path2D(z.path));
        }
      });
      const [bx, by, bw, bh] = z.box;
      const [x0, y0] = toScreen(bx, by);
      const spot = { key: key(0, i), x: x0 + (bw * s) / 2, y: y0 + (bh * s) / 2, rect: [x0, y0, bw * s, bh * s] };
      spots.push(spot);
      if (selection.has(spot.key)) marks.push(spot);
    });

    // labels with pointer lines, left or right of the mouse
    ctx.font = `12px ${font}`;
    ctx.textBaseline = "middle";
    mouseZones().forEach((z, i) => {
      const sp = spots[i];
      const [left] = toScreen(-14, 0);
      const [right] = toScreen(134, 0);
      const toX = z.side < 0 ? sp.rect[0] - 4 : sp.rect[0] + sp.rect[2] + 4;
      const lx = z.side < 0 ? left : right;
      ctx.strokeStyle = edge;
      ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(z.side < 0 ? lx + 6 : lx - 6, sp.y); ctx.lineTo(toX, sp.y); ctx.stroke();
      ctx.fillStyle = muted;
      ctx.textAlign = z.side < 0 ? "right" : "left";
      ctx.fillText(light.zones[i], lx, sp.y);
    });

    for (const m of marks) {
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.setLineDash([]);
      ctx.beginPath(); ctx.roundRect(m.rect[0] - 3, m.rect[1] - 3, m.rect[2] + 6, m.rect[3] + 6, 6); ctx.stroke();
    }
    drawDragBox(ctx);
  }

  // The graphics card: one light bar along its edge, in the card's color.
  function drawGpu(canvas) {
    const { ctx, w, h } = setupCanvas(canvas);
    const css = getComputedStyle(document.documentElement);
    const raised = css.getPropertyValue("--raised").trim();
    const surface = css.getPropertyValue("--surface").trim();
    const edge = css.getPropertyValue("--line").trim();
    const c = frame.gpu[0] || "#000000";

    const cw = Math.min(w * 0.9, (h - 30) * 3.3), ch = cw / 3.3;
    const x = (w - cw) / 2, y = (h - ch) / 2;
    ctx.fillStyle = surface;
    ctx.strokeStyle = edge;
    ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.roundRect(x, y, cw, ch, ch * 0.08); ctx.fill(); ctx.stroke();
    // three fans
    for (let f = 0; f < 3; f++) {
      const cx = x + cw * (0.2 + f * 0.3), cy = y + ch * 0.55, r = ch * 0.34;
      ctx.fillStyle = raised;
      ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
      ctx.lineWidth = r * 0.12;
      for (let b = 0; b < 7; b++) {
        const a = (b / 7) * Math.PI * 2;
        ctx.beginPath(); ctx.arc(cx + Math.cos(a) * r * 0.25, cy + Math.sin(a) * r * 0.25, r * 0.5, a - 0.2, a + 0.8); ctx.stroke();
      }
      ctx.lineWidth = 1.5;
      ctx.fillStyle = surface;
      ctx.beginPath(); ctx.arc(cx, cy, r * 0.22, 0, Math.PI * 2); ctx.fill();
    }
    // light bar along the top edge
    const bar = [x + cw * 0.06, y + ch * 0.07, cw * 0.88, ch * 0.07];
    ctx.shadowColor = c;
    ctx.shadowBlur = 22;
    ctx.fillStyle = c;
    ctx.beginPath(); ctx.roundRect(...bar, bar[3] / 2); ctx.fill();
    ctx.shadowBlur = 0;
    spots = [{ key: key(0, 0), x: x + cw / 2, y: y + ch / 2, rect: [x, y, cw, ch] }];
    if (selection.has(spots[0].key)) {
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.beginPath(); ctx.roundRect(x - 4, y - 4, cw + 8, ch + 8, ch * 0.1); ctx.stroke();
    }
  }

  // The motherboard: a row for the built-in lights, then one per ARGB header.
  function drawBoard(canvas) {
    const { ctx, w, h } = setupCanvas(canvas);
    const css = getComputedStyle(document.documentElement);
    const muted = css.getPropertyValue("--muted").trim();
    const edge = css.getPropertyValue("--line").trim();
    const font = css.getPropertyValue("--font-mono").trim();
    const accent = css.getPropertyValue("--accent").trim();
    const lens = boardLens();
    spots = [];
    if (!lens.length) return;

    const labelW = Math.min(150, w * 0.3);
    const rowH = Math.min(70, h / lens.length);
    const top = (h - rowH * lens.length) / 2;
    const marks = [];
    ctx.font = `12px ${font}`;
    ctx.textBaseline = "middle";
    lens.forEach((n, c) => {
      const cy = top + rowH * (c + 0.5);
      ctx.fillStyle = muted;
      ctx.textAlign = "left";
      ctx.fillText(c === 0 ? "Board lights" : headerName(c - 1), 0, cy);
      if (!n) {
        ctx.fillText("no LEDs set", labelW, cy);
        return;
      }
      const room = w - labelW - 8;
      const step = Math.min(34, room / n);
      const r = Math.max(2.5, Math.min(11, step * 0.36));
      ctx.strokeStyle = edge;
      ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(labelW, cy); ctx.lineTo(labelW + step * n, cy); ctx.stroke();
      for (let i = 0; i < n; i++) {
        const cx = labelW + step * (i + 0.5);
        const color = (frame.board[c] || [])[i] || "#000000";
        ctx.shadowColor = color;
        ctx.shadowBlur = 14;
        ctx.fillStyle = color;
        ctx.beginPath(); ctx.arc(cx, cy, r, 0, Math.PI * 2); ctx.fill();
        ctx.shadowBlur = 0;
        const s = { key: key(c, i), x: cx, y: cy, rect: [cx - step / 2, cy - rowH / 2, step, rowH] };
        spots.push(s);
        if (selection.has(s.key)) marks.push([cx, cy, r]);
      }
    });
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = 2;
    for (const [x, y, r] of marks) { ctx.beginPath(); ctx.arc(x, y, r + 4, 0, Math.PI * 2); ctx.stroke(); }

    drawDragBox(ctx);
  }

  function setupCanvas(canvas) {
    const dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    $("fan-labels").hidden = true;
    return { ctx, w, h };
  }

  // How many LEDs are on each ARGB header. The board can't tell, so it's a
  // setting; extra LEDs past the count are turned off.
  function renderArgb() {
    const bar = $("argb-bar");
    const headers = dev === "board" && light.board ? light.board.argb_headers : 0;
    bar.hidden = !headers;
    if (!headers) return;
    const lens = boardLens();
    const max = light.board.max_argb_leds;
    const setCount = (h, n) => {
      const counts = boardLens().slice(1);
      counts[h] = Math.max(0, Math.min(max, Math.round(Number(n) || 0)));
      light.store.argb_leds = counts;
      changed();
    };
    const rows = Array.from({ length: headers }, (_, h) => {
      const row = el("div", "argb-row");
      const step = el("div", "stepper");
      const minus = el("button", null, "−");
      const plus = el("button", null, "+");
      const input = el("input");
      input.type = "number";
      input.min = "0";
      input.max = String(max);
      input.value = String(lens[h + 1]);
      input.setAttribute("aria-label", `LEDs on ${headerName(h)}`);
      minus.setAttribute("aria-label", "One fewer LED");
      plus.setAttribute("aria-label", "One more LED");
      minus.addEventListener("click", () => setCount(h, lens[h + 1] - 1));
      plus.addEventListener("click", () => setCount(h, lens[h + 1] + 1));
      input.addEventListener("change", () => setCount(h, input.value));
      step.append(minus, input, plus);
      row.append(el("span", "nm", `LEDs on ${headerName(h)}`), step);
      return row;
    });
    bar.replaceChildren(...rows, el("p", "hint", "Not sure how many? Add a Static white layer, then raise the number until every LED on the device is lit."));
  }

  // Short forms for when a key is too narrow for its full label.
  const SHORT_LABELS = { Backspace: "Bksp", Enter: "Ent", Shift: "Shft", Caps: "Cap", Menu: "Mnu", Ctrl: "Ctl", Space: "Spc" };

  // Draws a key's label as large as fits (down to 7px), switching to a
  // short form if needed, and never outside the key.
  function drawKeyLabel(ctx, label, x, y, kw, kh, u, color, font) {
    const room = kw - 6;
    const start = Math.max(9, u * 1.15);
    let text = label, size = start;
    const fits = (t, px) => {
      ctx.font = `${px}px ${font}`;
      return ctx.measureText(t).width <= room;
    };
    while (size > 7 && !fits(text, size)) size -= 0.5;
    if (!fits(text, size) && SHORT_LABELS[label]) {
      text = SHORT_LABELS[label];
      size = start;
      while (size > 7 && !fits(text, size)) size -= 0.5;
    }
    ctx.save();
    ctx.beginPath();
    ctx.rect(x + 2, y + 2, kw - 4, kh - 4);
    ctx.clip();
    ctx.fillStyle = color;
    ctx.font = `${size}px ${font}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(text, x + kw / 2, y + kh / 2 - 2);
    ctx.restore();
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
    t.style.background = thumbCss(l.effect, l.color1, l.color2, fx.more ? l.palette || [] : []);
    t.style.setProperty("--off", l.color2);
    t.style.setProperty("--period", `${(2 + (l.palette || []).length) * 12}px`);
    $("effect-name").textContent = fx.name;
    $("effect-desc").textContent = fx.desc;
    renderRippleNote(l);

    $("target-desc").textContent = target;
    $("target-all").disabled = l.target.kind === "all";
    $("target-all").textContent = describeTarget({ kind: "all" });

    document.querySelectorAll("[data-spread]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.spread === l.spread)));
    document.querySelectorAll("[data-reverse]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.reverse === String(!!l.reverse))));
    const SPREAD = { node: ["Across fans", "Per fan"], keyboard: ["Left to right", "Top to bottom"], mouse: ["Front to back", "Back to front"], gpu: ["", ""], board: ["Across everything", "Along each strip"] };
    $("spread-across").textContent = SPREAD[dev][0];
    $("spread-per-fan").textContent = SPREAD[dev][1];
    $("field-colors").hidden = !fx.c1;
    $("field-spread").hidden = !fx.spread || dev === "gpu"; // one light: nothing to spread over
    $("field-direction").hidden = !fx.direction || dev === "gpu";
    const bass = fx.bass ? BASS[dev] : [];
    $("field-bass").hidden = !bass.length;
    $("bass-options").replaceChildren(...bass.map(([name, spread, reverse]) => {
      const b = el("button", null, name);
      b.setAttribute("aria-pressed", String(l.spread === spread && !!l.reverse === reverse));
      b.addEventListener("click", () => editLayer({ spread, reverse }));
      return b;
    }));
    $("field-speed").hidden = !fx.speed;
    $("slot-2").hidden = !fx.c2;
    $("slot-1-name").textContent = fx.c1 || "";
    $("slot-2-name").textContent = fx.c2 || "";
    if (!fx.c2 && activeSlot === "color2") activeSlot = "color1";
    $("color-slots").hidden = false;
    $("picker").hidden = false;
    $("quick-colors").hidden = false;
    renderPalette(l, fx);
    for (const k of ["color1", "color2"]) {
      $(`${k}-chip`).style.setProperty("--chip", l[k]);
      $(`${k}-hex`).textContent = l[k];
    }
    $("slot-1").classList.toggle("active", activeSlot === "color1");
    $("slot-2").classList.toggle("active", activeSlot === "color2");
    $("slot-1").setAttribute("aria-selected", String(activeSlot === "color1"));
    $("slot-2").setAttribute("aria-selected", String(activeSlot === "color2"));
    if (fx.c1 && !(fx.random && l.random)) syncPicker();

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
    const hex = slotColor();
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

  // The color being edited: "color1", "color2", or "p0", "p1"... for a
  // layer's added colors.
  function slotColor() {
    const l = layer();
    if (activeSlot.startsWith("p")) return (l.palette || [])[Number(activeSlot.slice(1))] || l.color1;
    return l[activeSlot];
  }

  // The layer change that sets the edited color.
  function slotPatch(hex) {
    if (!activeSlot.startsWith("p")) return { [activeSlot]: hex };
    const palette = [...(layer().palette || [])];
    palette[Number(activeSlot.slice(1))] = hex;
    return { palette };
  }

  function pickHsv(next) {
    hsv = { ...hsv, ...next };
    editLayer(slotPatch(hsvToHex(hsv)));
  }

  // A layer's added colors: one slot each, plus Add; or, for typing
  // effects, random colors.
  function renderPalette(l, fx) {
    const row = $("palette-row");
    const random = !!fx.random && !!l.random;
    row.hidden = !fx.more || random;
    $("random-row").hidden = !fx.random;
    $("random-hint").hidden = !random;
    $("random-colors").checked = random;
    if (!fx.more) return;
    if (random) {
      $("color-slots").hidden = true;
      $("picker").hidden = true;
      $("quick-colors").hidden = true;
      return;
    }
    const palette = l.palette || [];
    if (activeSlot.startsWith("p") && Number(activeSlot.slice(1)) >= palette.length) activeSlot = "color1";
    row.replaceChildren(...palette.map((c, i) => {
      const b = el("button", "slot" + (activeSlot === `p${i}` ? " active" : ""));
      const chip = el("span", "chip");
      chip.style.setProperty("--chip", c);
      const txt = el("span");
      txt.append(el("span", "slot-name", `${fx.more.name} ${i + fx.more.from}`), el("span", "slot-hex", c));
      const x = el("span", "chip-x", "×");
      x.title = "Remove this color";
      x.addEventListener("click", (e) => {
        e.stopPropagation();
        const next = palette.filter((_, j) => j !== i);
        activeSlot = "color1";
        hsv = null;
        editLayer({ palette: next });
      });
      b.append(chip, txt, x);
      b.addEventListener("click", () => { activeSlot = `p${i}`; hsv = null; syncEditor(); });
      return b;
    }), ...(palette.length < 15 ? [(() => {
      const add = el("button", "slot palette-add", "+ Add a color");
      add.addEventListener("click", () => {
        const pick = QUICK_COLORS[(palette.length + 1) % QUICK_COLORS.length][1];
        activeSlot = `p${palette.length}`;
        hsv = null;
        editLayer({ palette: [...palette, pick] });
      });
      return add;
    })()] : []));
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
        editLayer(slotPatch(v.toLowerCase()));
      }
    });
    input.addEventListener("blur", () => layer() && syncPicker());
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") input.blur(); });
  }

  // Ripples only show on devices that have their own ripple layer, so say
  // which ones do.
  const RIPPLE_DEVICES = [["layers", "Lighting controller (fans)"], ["keyboard", "Keyboard"], ["mouse", "Mouse"], ["gpu", "Graphics card"], ["motherboard", "Motherboard"]];
  function renderRippleNote(l) {
    const box = $("ripple-note");
    box.hidden = l.effect !== "ripple";
    if (box.hidden) return;
    const p = profile();
    const has = RIPPLE_DEVICES.filter(([f]) => (p[f] || []).some((x) => x.enabled && x.effect === "ripple")).map(([, n]) => n);
    const missing = RIPPLE_DEVICES.filter(([, n]) => !has.includes(n)).map(([, n]) => n);
    const list = (names) => names.join(", ");
    box.replaceChildren(
      el("p", null, "The rings only appear on devices that have their own Typing ripple layer. Prism doesn't add it to the others for you."),
      el("p", null, `On: ${has.length ? list(has) : "none yet"}`),
      ...(missing.length ? [el("p", null, `Not on yet: ${list(missing)}. To add it, pick that device on the left and add a Typing ripple layer there too.`)] : []),
    );
  }

  /* ---------- wiring ---------- */

  function renderAll() {
    if (!light) return;
    $("node-lighting").classList.toggle("keyboard", dev === "keyboard");
    $("node-lighting").classList.toggle("mouse", dev === "mouse");
    $("node-lighting").classList.toggle("gpu", dev === "gpu");
    $("node-lighting").classList.toggle("board", dev === "board");
    renderArgb();
    renderChannels();
    $("select-label").textContent = `Select ${unit(2)}`;
    $("select-hint").textContent = {
      node: "Click LEDs in the preview, or drag a box around them. Hold Shift to add more.",
      keyboard: "Click keys on the keyboard, or drag a box around them. Hold Shift to add more.",
      mouse: "Click a zone on the mouse. Hold Shift to add more.",
      gpu: "The whole card shows one color at a time, so effects play over time rather than across it.",
      board: "Click LEDs, or drag a box around them. Hold Shift to add more.",
    }[dev];
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
        mouse: [newLayer("gradient", { name: "Ember fade" })],
        gpu: [newLayer("gradient", { name: "Ember fade" })],
        motherboard: [newLayer("gradient", { name: "Ember fade" })],
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
    light = { store: state.store, layout: state.layout, keys: state.keys, zones: state.mouse_zones, catalog: state.catalog, bindingsError: state.bindings_error, mouseButtons: state.mouse_buttons, board: state.board, pieces: state.pieces, node: state.node };
    syncLayout();
    layerIndex = Math.max(0, stack().length - 1);

    $("quick-colors").replaceChildren(...QUICK_COLORS.map(([n, hex]) => swatch(n, hex, () => { hsv = null; editLayer(slotPatch(hex)); })));
    $("random-colors").addEventListener("change", (e) => { activeSlot = "color1"; hsv = null; editLayer({ random: e.target.checked }); });
    $("paint-swatches").replaceChildren(...QUICK_COLORS.map(([n, hex]) => swatch(`Paint ${n.toLowerCase()}`, hex, () => paint(hex))));

    document.querySelectorAll("[data-spread]").forEach((b) => b.addEventListener("click", () => editLayer({ spread: b.dataset.spread })));
    document.querySelectorAll("[data-reverse]").forEach((b) => b.addEventListener("click", () => editLayer({ reverse: b.dataset.reverse === "true" })));
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
    // Changed from the tray menu (profile, bindings on/off).
    await listen("store-updated", (e) => {
      light.store = e.payload;
      syncLayout();
      layerIndex = Math.max(0, stack().length - 1);
      hsv = null;
      renderAll();
      listeners.forEach((fn) => fn());
    });
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

  return {
    init,
    show,
    // For the Key bindings screen, which shares profiles with lighting.
    state: () => light,
    profile: () => profile(),
    save: () => { send(); renderProfiles(); },
    onChange: (fn) => listeners.push(fn),
    drawKeyLabel,
  };
})();
