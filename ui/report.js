// Prism "Help support this device": a step-by-step report the person can
// attach to a GitHub issue. Every piece of data is shown before it's saved,
// sections can be left out, and Prism never sends the report anywhere.
// Uses `invoke`, `$` and `el` from app.js.
"use strict";

const Report = (() => {
  let device = null;   // the device being reported (from the device list)
  let facts = null;    // read-only device facts
  let presses = [];    // [code, name] pressed during the button walk
  let lights = [];     // light walk answers
  let colors = [];     // color check answers
  let step = "intro";
  let capturing = false;
  let unlisten = null;

  const MOUSE_PARTS = ["Front", "Scroll wheel", "Logo", "DPI light", "Side", "Back / underglow", "Thumb", "Something else", "Nothing lit"];
  const CHECK_COLORS = [["red", "#ff0000"], ["green", "#00ff00"], ["blue", "#0000ff"], ["white", "#ffffff"]];
  const NODE_TEST_LEDS = 60;

  // Which light walk this device can do: Prism can only light devices whose
  // protocol it already speaks.
  function lightKind() {
    if (!device || !device.recognized) return null;
    if (device.kind === "mouse") return "mouse";
    if (device.kind === "lighting") return "node";
    return null;
  }

  /* ---------- the window ---------- */

  function open(d) {
    device = d;
    facts = null;
    presses = [];
    lights = [];
    colors = [];
    step = "intro";
    $("report-overlay").hidden = false;
    render();
  }

  async function close() {
    await stopCapture();
    await invoke("report_light_off").catch(() => {});
    $("report-overlay").hidden = true;
  }

  function frame(title, sub, body, actions) {
    const head = el("div", "fx-head");
    const t = el("div");
    t.append(el("h2", null, title), el("p", "hint", sub));
    const x = el("button", "mini", "Close");
    x.addEventListener("click", close);
    head.append(t, x);
    const foot = el("div", "onboard-actions report-actions");
    foot.append(...actions);
    $("report-dialog").replaceChildren(head, ...body, foot);
  }

  const button = (cls, text, fn) => {
    const b = el("button", cls, text);
    b.addEventListener("click", fn);
    return b;
  };
  const list = (items) => {
    const ul = el("ul", "report-list");
    items.forEach((i) => ul.append(typeof i === "string" ? el("li", null, i) : i));
    return ul;
  };
  const steps = () => ["intro", "facts", "buttons", ...(lightKind() ? ["lights"] : []), "review"];
  const stepNo = () => `Step ${steps().indexOf(step) + 1} of ${steps().length}`;

  function render() {
    ({ intro, facts: showFacts, buttons, lights: lightWalk, review })[step]();
  }

  function go(next) {
    step = next;
    render();
  }

  /* ---------- 1. what this is ---------- */

  function intro() {
    frame("Help support this device", `${device.name} · ${stepNo()}`, [
      el("p", "report-text", `This makes a report about your ${device.name}, so it can be supported in Prism. You'll see every piece of it before it's saved, and you can leave parts out.`),
      el("p", "report-strong", "Prism never sends the report anywhere. It saves a file; you decide whether to attach it to a GitHub issue."),
      el("span", "label", "What it can include"),
      list([
        "Device facts: its USB ID, product name and maker, how it describes its messages (a technical description from the device), which keys and buttons Linux says it has, and the Prism and Linux versions.",
        "The keys or buttons you press during the button test: only on this device, and only while that step is open.",
        ...(lightKind() ? ["What lit up during the light test, and your answers."] : []),
        "Any notes you type at the end.",
      ]),
      el("span", "label", "Never included"),
      list([
        "The device's serial number (it's removed).",
        "Your name, username, computer name, files or folders.",
        "Anything you type outside the button test, or on any other device.",
        "Anything about your network or other hardware.",
      ]),
    ], [button("btn", "Start", () => go("facts")), button("mini", "Cancel", close)]);
  }

  /* ---------- 2. device facts (read-only) ---------- */

  async function showFacts() {
    frame("Device facts", `${stepNo()} · read from Linux; nothing is sent to the device`, [el("p", "hint", "Reading…")], []);
    if (!facts) {
      try {
        facts = await invoke("report_facts", { usbId: device.usb_id });
      } catch (e) {
        frame("Device facts", stepNo(), [el("p", "report-text", `Couldn't read the device: ${e}`)], [button("mini", "Close", close)]);
        return;
      }
    }
    const rows = [
      ["USB ID", facts.usb_id],
      ["Product name", facts.product || "(none given)"],
      ["Maker", facts.manufacturer || "(none given)"],
      ["Serial number", facts.serial_number],
      ["Prism knows it as", facts.prism_knows],
      ["Looks like", facts.looks_like],
      ["Interfaces", `${facts.interfaces.length} (with ${facts.interfaces.filter((i) => i.descriptor).length} message descriptions)`],
      ["Keys and buttons Linux lists", `${facts.input_devices.reduce((t, d) => t + d.can_send.length, 0)} (what it can send, not anything pressed)`],
      ["Prism version", facts.prism_version],
      ["Linux version", facts.linux_version],
    ];
    const table = el("div", "report-facts");
    for (const [k, v] of rows) table.append(el("span", "report-key", k), el("span", null, v));
    frame("Device facts", `${stepNo()} · read from Linux; nothing is sent to the device`, [
      el("p", "report-text", "This is what Linux already knows about the device. You'll see the full details, including the technical parts, on the last step."),
      table,
    ], [button("btn", "Next", () => go("buttons")), button("mini", "Back", () => go("intro"))]);
  }

  /* ---------- 3. button walk ---------- */

  async function startCapture() {
    if (capturing) return;
    capturing = true;
    const { listen } = window.__TAURI__.event;
    unlisten = await listen("report-press", (e) => {
      if (!presses.some(([c]) => c === e.payload.code)) presses.push([e.payload.code, e.payload.name]);
      if (step === "buttons") renderPresses();
    });
    await invoke("report_capture_start", { usbId: device.usb_id });
  }

  async function stopCapture() {
    if (!capturing) return;
    capturing = false;
    await invoke("report_capture_stop").catch(() => {});
    if (unlisten) unlisten();
    unlisten = null;
  }

  function renderPresses() {
    const box = $("report-presses");
    if (!box) return;
    box.replaceChildren(...(presses.length ? presses.map(([code, name], i) => {
      const chip = el("span", "channel-chip", `${name} (${code})`);
      const x = el("button", "chip-x", "×");
      x.title = "Leave this one out";
      x.addEventListener("click", () => { presses.splice(i, 1); renderPresses(); });
      chip.append(x);
      return chip;
    }) : [el("span", "hint", "Nothing pressed yet.")]));
  }

  function buttons() {
    const isKeyboard = device.kind === "keyboard";
    frame("Button test", `${stepNo()} · optional`, [
      el("p", "report-text", isKeyboard
        ? `Press each key on your ${device.name} once, including Fn combinations and media keys. Each one appears below.`
        : `Press each button on your ${device.name} once, including DPI and side buttons. Each one appears below.`),
      el("p", "report-strong", "Only presses on this device, and only while this step is open, are recorded. Each key is listed once, not in the order you typed, so don't type anything private here."),
      el("div", "channel-chips report-presses", ""),
      el("p", "hint", "Buttons the device handles by itself (some DPI or profile buttons) may not appear; that's useful to know too. Remove any you didn't mean to press with ×."),
    ], [
      button("btn", "Next", async () => { await stopCapture(); go(lightKind() ? "lights" : "review"); }),
      button("mini", "Skip this step", async () => { await stopCapture(); presses = []; go(lightKind() ? "lights" : "review"); }),
      button("mini", "Back", async () => { await stopCapture(); go("facts"); }),
    ]);
    $("report-dialog").querySelector(".report-presses").id = "report-presses";
    renderPresses();
    startCapture();
  }

  /* ---------- 4. light walk ---------- */

  async function lightWalk() {
    const kind = lightKind();
    const items = kind === "mouse" ? [1, 2, 3, 4, 5, 6] : [0, 1];
    let i = lights.length;
    // the walk, one zone or channel at a time
    if (i < items.length) {
      const item = items[i];
      if (kind === "mouse") await invoke("report_light_mouse", { zone: item, color: "#ffffff" });
      else await invoke("report_light_node", { channel: item, color: "#808080", leds: NODE_TEST_LEDS });
      const answer = (a) => { lights.push(kind === "mouse" ? { zone_id: item, lit: a } : { channel: item + 1, lit: a }); lightWalk(); };
      let body;
      if (kind === "mouse") {
        const opts = el("div", "report-options");
        MOUSE_PARTS.forEach((p) => opts.append(button("mini", p, () => answer(p))));
        body = [el("p", "report-text", `Zone number ${item} is lit white (the others are off). Which part of the mouse lit up?`), opts];
      } else {
        const n = el("input", "report-number");
        n.type = "number"; n.min = "0"; n.max = "30"; n.placeholder = "0";
        const opts = el("div", "report-options");
        opts.append(button("mini", "Nothing lit", () => answer("nothing")), n, button("btn", "That many", () => answer(`${Number(n.value) || 0} fans or strips`)));
        body = [el("p", "report-text", `Channel ${item + 1} is lit grey (the first ${NODE_TEST_LEDS} LEDs). How many fans or strips lit up?`), opts];
      }
      frame("Light test", `${stepNo()} · ${i + 1} of ${items.length} · only lighting colors are sent`, body, [
        button("mini", "Skip the light test", async () => { await invoke("report_light_off"); lights = []; colors = []; go("review"); }),
      ]);
      return;
    }
    // then the color check, on everything at once
    const j = colors.length;
    if (j < CHECK_COLORS.length) {
      const [name, hex] = CHECK_COLORS[j];
      if (kind === "mouse") await invoke("report_light_mouse", { zone: null, color: hex });
      else await invoke("report_light_node", { channel: null, color: hex, leds: NODE_TEST_LEDS });
      const answer = (a) => { colors.push({ color: name, looked_right: a }); lightWalk(); };
      frame("Color check", `${stepNo()} · ${j + 1} of ${CHECK_COLORS.length}`, [
        el("p", "report-text", `Everything should now be ${name}. Does it look ${name}?`),
        (() => { const o = el("div", "report-options"); o.append(button("mini", "Yes", () => answer("yes")), button("mini", "No, a different color", () => answer("no")), button("mini", "Nothing lit", () => answer("nothing lit"))); return o; })(),
      ], []);
      return;
    }
    await invoke("report_light_off");
    go("review");
  }

  /* ---------- 5. review and save ---------- */

  let include = { facts: true, descriptors: true, linux: true, presses: true, lights: true };

  function buildReport(notes) {
    const r = { prism_device_report: 1 };
    if (include.facts) {
      const f = { ...facts };
      if (!include.linux) delete f.linux_version;
      if (!include.descriptors) f.interfaces = f.interfaces.map((i) => ({ ...i, descriptor: null }));
      r.device = f;
    } else {
      r.device = { usb_id: facts.usb_id };
    }
    if (include.presses && presses.length) r.button_test = presses.map(([code, name]) => ({ code, name }));
    if (include.lights && (lights.length || colors.length)) r.light_test = { zones: lights, color_check: colors };
    if (notes.trim()) r.notes = notes.trim().slice(0, 2000);
    return r;
  }

  function review() {
    const notes = el("textarea", "report-notes");
    notes.placeholder = "Optional: anything else, such as the exact model name printed under the device.";
    notes.maxLength = 2000;
    const pre = el("pre", "report-json");
    const update = () => { pre.textContent = JSON.stringify(buildReport(notes.value), null, 2); };
    notes.addEventListener("input", update);

    const toggles = el("div", "report-toggles");
    const toggle = (keyName, label, available = true) => {
      if (!available) return;
      const l = el("label", "report-toggle");
      const c = el("input");
      c.type = "checkbox";
      c.checked = include[keyName];
      c.addEventListener("change", () => { include[keyName] = c.checked; update(); });
      l.append(c, el("span", null, label));
      toggles.append(l);
    };
    toggle("facts", "Device facts (without them, only the USB ID is kept)");
    toggle("descriptors", "The device's technical message descriptions (most useful for adding support)");
    toggle("linux", `Linux version (${facts.linux_version})`);
    toggle("presses", `Button test: ${presses.length} keys or buttons`, presses.length > 0);
    toggle("lights", "Light test answers", lights.length + colors.length > 0);

    const status = el("p", "hint", "");
    const save = button("btn", "Save report", async () => {
      save.disabled = true;
      try {
        const path = await invoke("report_save", { json: JSON.stringify(buildReport(notes.value)), usbId: device.usb_id });
        status.textContent = "";
        frame("Saved", "Nothing has been sent anywhere", [
          el("p", "report-text", "Your report is saved as:"),
          el("p", "report-path", path),
          el("p", "report-text", "To share it, open a new issue on Prism's GitHub page and drag the file into it. You can add comments there too. You can also delete the file if you change your mind."),
        ], [
          button("btn", "Open the GitHub issue page", () => invoke("report_open_issue", { product: device.name }).catch(() => {})),
          button("mini", "Done", close),
        ]);
      } catch (e) {
        status.textContent = `Couldn't save: ${e}`;
        save.disabled = false;
      }
    });

    frame("Review your report", `${stepNo()} · this is exactly what will be saved`, [
      el("p", "report-text", "Untick anything you'd rather leave out. The text below is the complete file, word for word."),
      toggles,
      notes,
      pre,
      status,
    ], [save, button("mini", "Cancel", close)]);
    update();
  }

  /* ---------- other RGB hardware (read-only, no tests) ---------- */

  let other = null;   // what was found
  let picked = null;  // which items to include

  function openOther() {
    device = { name: "other RGB hardware", usb_id: "other", kind: "other", recognized: false };
    other = null;
    $("report-overlay").hidden = false;
    otherIntro();
  }

  function otherIntro() {
    frame("Report other RGB hardware", "Step 1 of 3", [
      el("p", "report-text", "This makes a report about lighting hardware that isn't a Corsair USB device, such as your graphics card, motherboard lighting or another brand's keyboard or mouse, so support for it can be added. You'll see every piece before it's saved, and you choose what's included."),
      el("p", "report-strong", "Prism only reads what Linux already knows. Nothing is sent to any of the hardware, and Prism never sends the report anywhere: it saves a file, and you decide whether to share it."),
      el("span", "label", "What it can include"),
      list([
        "USB devices from makers that often have lighting (ASUS, MSI, Gigabyte, NZXT, Razer, Logitech, SteelSeries and others): USB ID, product name, maker, and how each describes its messages.",
        "Graphics cards: their PCI IDs (chip maker and model, board maker and model) and the names of their I2C buses.",
        "Your motherboard's maker and model name.",
        "The names of the motherboard's internal buses (for example \"SMBus PIIX4\"). Nothing on them is probed.",
        "Prism and Linux versions, and any notes you add.",
      ]),
      el("span", "label", "Never included"),
      list([
        "Serial numbers (removed), or your computer's unique ID.",
        "Your name, username, computer name, files or folders.",
        "Anything you type, or anything about your network.",
        "Anything found by probing buses or sending commands: Prism doesn't do either here.",
      ]),
    ], [button("btn", "Start", otherFound), button("mini", "Cancel", close)]);
  }

  async function otherFound() {
    frame("What Prism found", "Step 2 of 3 · read from Linux only", [el("p", "hint", "Looking…")], []);
    if (!other) {
      try {
        other = await invoke("report_other_hardware");
      } catch (e) {
        frame("What Prism found", "Step 2 of 3", [el("p", "report-text", `Couldn't look: ${e}`)], [button("mini", "Close", close)]);
        return;
      }
      picked = { usb: other.usb_devices.map(() => true), gpu: other.graphics_cards.map(() => true), board: !!other.motherboard, buses: true, linux: true };
    }
    const box = el("div", "report-toggles");
    const item = (label, sub, get, set) => {
      const l = el("label", "report-toggle report-item");
      const c = el("input");
      c.type = "checkbox";
      c.checked = get();
      c.addEventListener("change", () => set(c.checked));
      const t = el("span");
      t.append(el("span", "report-item-name", label), el("span", "hint", sub));
      l.append(c, t);
      box.append(l);
    };
    other.usb_devices.forEach((d, i) => item(`${d.product || "USB device"} (${d.usb_id})`, `USB · ${d.maker_guess} · ${d.interfaces.length} interface(s)`, () => picked.usb[i], (v) => (picked.usb[i] = v)));
    other.graphics_cards.forEach((g, i) => item(`Graphics card ${g.pci_id}, board ${g.subsystem_id}`, g.prism_supports ? "Already supported by Prism" : `${g.i2c_buses.length} I2C bus(es)`, () => picked.gpu[i], (v) => (picked.gpu[i] = v)));
    if (other.motherboard) item(`Motherboard: ${other.motherboard.maker} ${other.motherboard.model}`, "Maker and model name only", () => picked.board, (v) => (picked.board = v));
    if (other.system_buses.length) item("Motherboard bus names", other.system_buses.join(", "), () => picked.buses, (v) => (picked.buses = v));
    item(`Linux version (${other.linux_version})`, "Helps if a problem depends on the Linux version", () => picked.linux, (v) => (picked.linux = v));
    const nothing = !other.usb_devices.length && !other.graphics_cards.length && !other.motherboard;
    frame("What Prism found", "Step 2 of 3 · read from Linux only", [
      el("p", "report-text", nothing ? "Prism didn't find any lighting hardware it could report." : "Untick anything you'd rather leave out. Pick at least the hardware you'd like supported."),
      box,
    ], [button("btn", "Next", otherReview), button("mini", "Back", otherIntro)]);
  }

  function buildOther(notes) {
    const r = { prism_hardware_report: 1, prism_version: other.prism_version };
    if (picked.linux) r.linux_version = other.linux_version;
    r.usb_devices = other.usb_devices.filter((_, i) => picked.usb[i]);
    r.graphics_cards = other.graphics_cards.filter((_, i) => picked.gpu[i]);
    if (picked.board && other.motherboard) r.motherboard = other.motherboard;
    if (picked.buses && other.system_buses.length) r.system_buses = other.system_buses;
    if (notes.trim()) r.notes = notes.trim().slice(0, 2000);
    return r;
  }

  function otherReview() {
    const notes = el("textarea", "report-notes");
    notes.placeholder = "Optional: which of these has lighting you'd like supported, and anything else that helps.";
    notes.maxLength = 2000;
    const pre = el("pre", "report-json");
    const update = () => { pre.textContent = JSON.stringify(buildOther(notes.value), null, 2); };
    notes.addEventListener("input", update);
    const status = el("p", "hint", "");
    const save = button("btn", "Save report", async () => {
      save.disabled = true;
      try {
        const path = await invoke("report_save", { json: JSON.stringify(buildOther(notes.value)), usbId: "other" });
        frame("Saved", "Nothing has been sent anywhere", [
          el("p", "report-text", "Your report is saved as:"),
          el("p", "report-path", path),
          el("p", "report-text", "To share it, open a new issue on Prism's GitHub page and drag the file into it. You can also delete the file if you change your mind."),
        ], [
          button("btn", "Open the GitHub issue page", () => invoke("report_open_issue", { product: "other RGB hardware" }).catch(() => {})),
          button("mini", "Done", close),
        ]);
      } catch (e) {
        status.textContent = `Couldn't save: ${e}`;
        save.disabled = false;
      }
    });
    frame("Review your report", "Step 3 of 3 · this is exactly what will be saved", [
      el("p", "report-text", "The text below is the complete file, word for word. Go back to untick anything."),
      notes,
      pre,
      status,
    ], [save, button("mini", "Back", otherFound), button("mini", "Cancel", close)]);
    update();
  }

  function init() {
    $("report-overlay").addEventListener("click", (e) => { if (e.target === $("report-overlay")) close(); });
    $("report-other").addEventListener("click", openOther);
  }

  return { init, open, openOther };
})();
