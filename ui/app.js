// Prism screen logic. Talks to the Rust side only through `invoke`.
"use strict";

const { invoke } = window.__TAURI__.core;

const ICONS = {
  keyboard: '<svg viewBox="0 0 24 24"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></svg>',
  mouse: '<svg viewBox="0 0 24 24"><rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 7v4"/></svg>',
  lighting: '<svg viewBox="0 0 24 24"><rect x="4" y="8" width="16" height="8" rx="2"/><path d="M8 8V5M16 8V5M8 19v-3M16 19v-3"/></svg>',
  unknown: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 16h.01M10 9.5a2 2 0 1 1 3 1.7c-.6.4-1 .8-1 1.8"/></svg>',
};

const KIND_LABEL = { keyboard: "Keyboard", mouse: "Mouse", lighting: "Lighting controller", unknown: "Corsair device" };

// What each tab will do, shown until the feature is built.
const SOON = {
  light: "<b>Lighting control is next.</b> This preview shows Prism's colors, not what the device is showing right now.",
  keys: "<b>Key bindings are coming.</b> You'll be able to remap keys and buttons and record macros here.",
  mem: "<b>Onboard memory is coming.</b> You'll be able to save profiles to devices that support it, so they work without Prism running.",
};

// 60% keyboard layout: [label, width in quarter-keys]
const KB_ROWS = [
  [["Esc",4],["1",4],["2",4],["3",4],["4",4],["5",4],["6",4],["7",4],["8",4],["9",4],["0",4],["-",4],["=",4],["Bksp",8]],
  [["Tab",6],["Q",4],["W",4],["E",4],["R",4],["T",4],["Y",4],["U",4],["I",4],["O",4],["P",4],["[",4],["]",4],["\\",6]],
  [["Caps",7],["A",4],["S",4],["D",4],["F",4],["G",4],["H",4],["J",4],["K",4],["L",4],[";",4],["'",4],["Enter",9]],
  [["Shift",9],["Z",4],["X",4],["C",4],["V",4],["B",4],["N",4],["M",4],[",",4],[".",4],["/",4],["Shift",11]],
  [["Ctrl",5],["Win",5],["Alt",5],["",25],["Alt",5],["Fn",5],["Menu",5],["Ctrl",5]],
];

const $ = (id) => document.getElementById(id);
let devices = [];
let selected = 0;
let tab = "light";

// Blend from purple to hot pink, t = 0..1
function ember(t) {
  const a = [0x7c, 0x3a, 0xed], b = [0xff, 0x2d, 0x95];
  return `rgb(${a.map((v, i) => Math.round(v + (b[i] - v) * t)).join(",")})`;
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function renderList() {
  const list = $("device-list");
  list.replaceChildren();
  devices.forEach((d, i) => {
    const b = el("button", "dev");
    b.setAttribute("aria-current", String(i === selected));
    const ic = el("span", "ic");
    ic.innerHTML = ICONS[d.kind] || ICONS.unknown; // fixed SVG strings, never device data
    const txt = el("span");
    txt.append(el("span", "nm", d.name), el("span", d.recognized ? "st" : "st unknown", d.recognized ? "Connected" : "Not supported yet"));
    b.append(ic, txt);
    b.addEventListener("click", () => { selected = i; render(); });
    list.append(b);
  });
}

function renderVisual(d) {
  const v = $("visual");
  v.replaceChildren();
  if (d.kind === "keyboard") {
    const kb = el("div", "kb");
    KB_ROWS.forEach((row, r) => {
      let col = 0;
      row.forEach(([label, w]) => {
        const k = el("div", "key", label);
        k.style.gridColumn = `span ${w}`;
        k.style.gridRow = String(r + 1);
        k.style.setProperty("--glow", ember((col + w / 2) / 60));
        kb.append(k);
        col += w;
      });
    });
    v.append(kb);
  } else if (d.kind === "mouse") {
    const w = el("div", "mouse-wrap");
    w.innerHTML = `<svg viewBox="0 0 120 190" width="140" height="222" role="img" aria-label="Mouse lighting zones">
      <path d="M60 6c30 0 50 22 50 60v58c0 36-22 60-50 60S10 160 10 124V66C10 28 30 6 60 6z" fill="#251A28" stroke="#38283C" stroke-width="2"/>
      <path d="M60 6v58" stroke="#38283C" stroke-width="2"/>
      <rect x="55" y="26" width="10" height="24" rx="5" fill="${ember(0)}"/>
      <rect x="12" y="80" width="5" height="40" rx="2.5" fill="${ember(.4)}"/>
      <rect x="103" y="80" width="5" height="40" rx="2.5" fill="${ember(.6)}"/>
      <path d="M42 150c6 10 30 10 36 0" stroke="${ember(1)}" stroke-width="6" fill="none" stroke-linecap="round"/>
    </svg>`;
    v.append(w);
  } else if (d.kind === "lighting") {
    const w = el("div", "strip-wrap");
    ["Channel 1", "Channel 2"].forEach((label, c) => {
      w.append(el("div", "strip-label", label));
      const s = el("div", "strip");
      for (let i = 0; i < 24; i++) {
        const led = el("i");
        const col = ember(c ? 1 - i / 23 : i / 23);
        led.style.background = col;
        led.style.boxShadow = `0 0 10px -2px ${col}`;
        s.append(led);
      }
      w.append(s);
    });
    v.append(w);
  } else {
    v.append(el("p", "dev-sub", "Prism doesn't know this device yet."));
  }
}

function render() {
  renderList();
  const has = devices.length > 0;
  $("empty-state").hidden = has;
  $("device-view").hidden = !has;
  if (!has) return;
  const d = devices[Math.min(selected, devices.length - 1)];
  $("dev-title").textContent = d.name;
  $("dev-sub").textContent = `${KIND_LABEL[d.kind]} · USB ${d.usb_id}`;
  document.querySelectorAll(".tab").forEach((t) => t.setAttribute("aria-selected", String(t.dataset.t === tab)));

  const nodeLighting = d.kind === "lighting" && tab === "light";
  $("node-lighting").hidden = !nodeLighting;
  $("visual").hidden = nodeLighting;
  $("soon").hidden = nodeLighting;
  if (nodeLighting) {
    drawFans();
    return;
  }
  $("soon").innerHTML = SOON[tab];
  renderVisual(d);
}

/* ---------- Lighting Node Pro: lighting controls ---------- */

const PRESETS = [
  ["Ember purple", "#7c3aed"], ["Hot pink", "#ff2d95"], ["White", "#ffffff"], ["Red", "#ff1a1a"],
  ["Orange", "#ff7a00"], ["Green", "#22e55b"], ["Cyan", "#22d3ee"], ["Blue", "#2d5bff"],
];

let light = null;      // { config, layout, connected }
let frame = [];        // latest colors sent to the device, per channel
let activeSlot = "color1";
let sendQueued = false;

function sendConfig() {
  // Coalesce rapid slider moves into one update per screen refresh.
  if (sendQueued) return;
  sendQueued = true;
  requestAnimationFrame(async () => {
    sendQueued = false;
    await invoke("set_lighting", { config: light.config });
  });
}

function update(changes) {
  Object.assign(light.config, changes);
  syncControls();
  sendConfig();
}

function syncControls() {
  const c = light.config;
  const twoColors = c.effect !== "static";
  document.querySelectorAll("[data-effect]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.effect === c.effect)));
  document.querySelectorAll("[data-spread]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.spread === c.spread)));
  $("field-spread").hidden = !(c.effect === "gradient" || c.effect === "wave");
  $("field-speed").hidden = !(c.effect === "wave" || c.effect === "breathe");
  $("slot-2").hidden = !twoColors;
  $("slot-1-name").textContent = twoColors ? "Color 1" : "Color";
  if (!twoColors) activeSlot = "color1";
  for (const k of ["color1", "color2"]) {
    $(`${k}-chip`).style.setProperty("--chip", c[k]);
    $(`${k}-hex`).textContent = c[k];
  }
  $("slot-1").classList.toggle("active", activeSlot === "color1");
  $("slot-2").classList.toggle("active", activeSlot === "color2");
  $("slot-1").setAttribute("aria-selected", String(activeSlot === "color1"));
  $("slot-2").setAttribute("aria-selected", String(activeSlot === "color2"));
  syncPicker();
  $("brightness").value = c.brightness;
  $("brightness").style.setProperty("--fill", `${c.brightness}%`);
  $("brightness-val").textContent = `${c.brightness}%`;
  $("speed").value = c.speed;
  $("speed").style.setProperty("--fill", `${((c.speed - 1) / 9) * 100}%`);
  $("speed-val").textContent = c.speed;
}

/* ---------- Color picker ---------- */

// Hue/saturation/value of the color being edited. Kept separately from the
// hex value so the hue doesn't jump around when the color is grey or black.
let hsv = null;

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
  const hex = light.config[activeSlot];
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
  update({ [activeSlot]: hsvToHex(hsv) });
}

// Drag on an element; `move` gets the pointer position as 0..1 fractions.
function draggable(elem, move) {
  const at = (e) => {
    const r = elem.getBoundingClientRect();
    move(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)));
  };
  elem.addEventListener("pointerdown", (e) => { elem.setPointerCapture(e.pointerId); at(e); });
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
      update({ [activeSlot]: v.toLowerCase() });
    }
  });
  input.addEventListener("blur", syncPicker);
  input.addEventListener("keydown", (e) => { if (e.key === "Enter") input.blur(); });
}

/* Fan preview: draws exactly the colors the engine sent. */
function drawFans() {
  const canvas = $("fans");
  if (!light || canvas.offsetParent === null) return;
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth, h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr)) canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  // Every fan on every channel, in order, split into hub and ring LEDs.
  const fans = [];
  light.layout.forEach((ch, ci) => {
    const per = ch.inner + ch.outer;
    for (let f = 0; f < ch.fans; f++) {
      const leds = (frame[ci] || []).slice(f * per, (f + 1) * per);
      fans.push({ hub: leds.slice(0, ch.inner), ring: leds.slice(ch.inner), inner: ch.inner, outer: ch.outer });
    }
  });
  if (!fans.length) return;

  const css = getComputedStyle(document.documentElement);
  const body = css.getPropertyValue("--surface").trim();
  const edge = css.getPropertyValue("--line").trim();
  const raised = css.getPropertyValue("--raised").trim();
  const slot = w / fans.length;
  const R = Math.min(slot, h) * 0.44;

  // A ring of glowing light segments.
  const ring = (cx, cy, radius, width, colors, count) => {
    const step = (Math.PI * 2) / count;
    for (let k = 0; k < count; k++) {
      const color = colors[k] || "#000000";
      const a = -Math.PI / 2 + k * step;
      ctx.strokeStyle = color;
      ctx.shadowColor = color;
      ctx.shadowBlur = 18;
      ctx.lineWidth = width;
      ctx.beginPath(); ctx.arc(cx, cy, radius, a - step * 0.46, a + step * 0.46); ctx.stroke();
    }
    ctx.shadowBlur = 0;
  };

  fans.forEach((fan, i) => {
    const cx = slot * (i + 0.5), cy = h / 2;

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

    // outer light ring
    ring(cx, cy, R * 0.86, R * 0.1, fan.ring, fan.outer);

    // blades
    ctx.strokeStyle = edge;
    ctx.lineWidth = R * 0.05;
    for (let b = 0; b < 9; b++) {
      const a = (b / 9) * Math.PI * 2;
      ctx.beginPath();
      ctx.arc(cx + Math.cos(a) * R * 0.3, cy + Math.sin(a) * R * 0.3, R * 0.42, a - 0.2, a + 0.9);
      ctx.stroke();
    }

    // hub with its own light ring
    ctx.fillStyle = body;
    ctx.beginPath(); ctx.arc(cx, cy, R * 0.34, 0, Math.PI * 2); ctx.fill();
    ring(cx, cy, R * 0.26, R * 0.09, fan.hub, fan.inner);
    ctx.fillStyle = raised;
    ctx.beginPath(); ctx.arc(cx, cy, R * 0.15, 0, Math.PI * 2); ctx.fill();
  });

  const labels = $("fan-labels");
  if (labels.childElementCount !== fans.length) {
    labels.style.gridTemplateColumns = `repeat(${fans.length}, 1fr)`;
    labels.replaceChildren(...fans.map((_, i) => el("span", null, `Fan ${i + 1}`)));
  }
}

async function initLighting() {
  light = await invoke("get_lighting");

  $("presets").replaceChildren(...PRESETS.map(([name, hex]) => {
    const b = el("button", "preset");
    b.style.background = hex;
    b.title = name;
    b.setAttribute("aria-label", name);
    b.addEventListener("click", () => { hsv = null; update({ [activeSlot]: hex }); });
    return b;
  }));

  document.querySelectorAll("[data-effect]").forEach((b) => b.addEventListener("click", () => update({ effect: b.dataset.effect })));
  document.querySelectorAll("[data-spread]").forEach((b) => b.addEventListener("click", () => update({ spread: b.dataset.spread })));
  document.querySelectorAll("[data-slot]").forEach((b) => b.addEventListener("click", () => {
    activeSlot = b.dataset.slot;
    hsv = null; // re-read from the newly chosen color
    syncControls();
  }));
  initPicker();
  $("brightness").addEventListener("input", (e) => update({ brightness: Number(e.target.value) }));
  $("speed").addEventListener("input", (e) => update({ speed: Number(e.target.value) }));

  syncControls();

  const { listen } = window.__TAURI__.event;
  await listen("lighting-frame", (e) => { frame = e.payload; drawFans(); });
  window.addEventListener("resize", drawFans);
}

async function scan() {
  const btn = $("rescan");
  btn.disabled = true;
  btn.textContent = "Scanning…";
  try {
    devices = await invoke("list_devices");
    $("empty-state").classList.remove("error");
    $("empty-state").querySelector("h2").textContent = "No Corsair devices found";
  } catch (err) {
    devices = [];
    const box = $("empty-state");
    box.classList.add("error");
    box.querySelector("h2").textContent = "Couldn't check for devices";
    box.querySelector("p").textContent = String(err);
  }
  btn.disabled = false;
  btn.textContent = "Rescan devices";
  render();
}

document.querySelectorAll(".tab").forEach((t) => t.addEventListener("click", () => { tab = t.dataset.t; render(); }));
$("rescan").addEventListener("click", scan);
initLighting().then(scan);
