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
  // Fans have no buttons or onboard memory, so the lighting controller only gets Lighting.
  const onlyLighting = d.kind === "lighting";
  if (onlyLighting) tab = "light";
  document.querySelector(".tabs").hidden = onlyLighting;
  document.querySelectorAll(".tab").forEach((t) => t.setAttribute("aria-selected", String(t.dataset.t === tab)));

  const nodeLighting = (d.kind === "lighting" || d.kind === "keyboard") && tab === "light";
  $("node-lighting").hidden = !nodeLighting;
  $("visual").hidden = nodeLighting;
  $("soon").hidden = nodeLighting;
  if (nodeLighting) {
    Lighting.show(d.kind === "keyboard" ? "keyboard" : "node");
    return;
  }
  $("soon").innerHTML = SOON[tab];
  renderVisual(d);
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
Lighting.init().then(scan);
