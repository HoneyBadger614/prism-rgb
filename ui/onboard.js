// Prism Onboard memory screen: back up what's stored on the keyboard and
// mouse (read-only). Uses `invoke`, `$` and `el` from app.js.
"use strict";

const Onboard = (() => {
  let busy = false;

  function summary(d, label) {
    if (!d.read) return `${label}: not connected`;
    const n = d.files.length;
    const bytes = d.files.reduce((t, [, size]) => t + size, 0);
    const kb = bytes >= 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${bytes} bytes`;
    return `${label}: ${n} file${n === 1 ? "" : "s"} (${kb})${d.error ? " · some files couldn't be read" : ""}`;
  }

  const count = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

  // Describe what restoring would do, in plain words.
  function describePlan(p) {
    const lines = [];
    const dev = (d, label) => {
      if (!d.connected) return;
      if (d.error) lines.push(`${label}: couldn't compare (${d.error})`);
      else if (!d.write.length && !d.delete.length) lines.push(`${label}: already matches this backup`);
      else {
        let t = `${label}: ${count(d.write.length, "file")} to write back`;
        if (d.delete.length) t += `, ${count(d.delete.length, "extra file")} to remove`;
        t += ` (${d.same} already the same)`;
        lines.push(t);
      }
      if (d.cannot_remove.length) lines.push(`${label}: ${count(d.cannot_remove.length, "newer file")} not in the backup will stay (${d.cannot_remove.join(", ")}); Prism can't remove mouse files yet`);
    };
    dev(p.keyboard, "Keyboard");
    dev(p.mouse, "Mouse");
    if (!lines.length) lines.push("Neither device is connected.");
    return lines;
  }

  async function askRestore(b, row) {
    const box = el("div", "restore-box");
    box.append(el("p", "hint", "Comparing this backup with your devices…"));
    row.after(box);
    row.querySelector(".restore-btn").disabled = true;
    let plan;
    try {
      plan = await invoke("restore_plan", { name: b.name });
    } catch (e) {
      box.replaceChildren(el("p", "hint", `Couldn't compare: ${e}`));
      return;
    }
    const nothing = [plan.keyboard, plan.mouse].every((d) => !d.connected || (!d.write.length && !d.delete.length));
    const list = el("ul", "restore-lines");
    describePlan(plan).forEach((l) => list.append(el("li", null, l)));
    const yes = el("button", "btn", "Yes, restore");
    const no = el("button", "mini", nothing ? "Close" : "Cancel");
    const done = () => { box.remove(); row.querySelector(".restore-btn").disabled = false; };
    no.addEventListener("click", done);
    yes.addEventListener("click", async () => {
      yes.disabled = no.disabled = true;
      box.querySelector(".restore-q").textContent = "Restoring… (making a fresh backup first)";
      try {
        const r = await invoke("restore_backup", { name: b.name });
        const n = r.keyboard.written.length + r.mouse.written.length + r.keyboard.deleted.length;
        const failed = [...r.keyboard.failed, ...r.mouse.failed];
        box.replaceChildren(el("p", "restore-q", failed.length
          ? `Restored ${count(n, "change")}, but ${count(failed.length, "file")} failed: ${failed.join("; ")}. Your safety backup from ${r.safety_backup} is in the list.`
          : `Restored: ${count(n, "change")} written. A safety backup (${r.safety_backup}) was made first, so this can be undone.`));
        const ok = el("button", "mini", "Close");
        ok.addEventListener("click", done);
        box.append(ok);
      } catch (e) {
        box.replaceChildren(el("p", "restore-q", `Couldn't restore: ${e}`));
      }
      renderList();
    });
    box.replaceChildren(
      el("p", "restore-q", nothing ? "Nothing to restore:" : "Restoring this backup would change your devices:"),
      list,
      ...(nothing ? [] : [el("p", "hint", "Prism makes a fresh backup first, so you can undo this.")]),
      el("div", "onboard-actions"),
    );
    box.lastChild.append(...(nothing ? [no] : [yes, no]));
  }

  // "Are you sure?" under the row, then delete the backup folder.
  function askDelete(b, row) {
    if (row.nextElementSibling?.classList.contains("delete-box")) return;
    const box = el("div", "restore-box delete-box");
    const yes = el("button", "btn danger", "Yes, delete it");
    const no = el("button", "mini", "Cancel");
    const done = () => box.remove();
    no.addEventListener("click", done);
    yes.addEventListener("click", async () => {
      yes.disabled = no.disabled = true;
      try {
        await invoke("delete_backup", { name: b.name });
        renderList();
      } catch (e) {
        box.replaceChildren(el("p", "restore-q", `Couldn't delete it: ${e}`));
        const ok = el("button", "mini", "Close");
        ok.addEventListener("click", done);
        box.append(ok);
      }
    });
    const actions = el("div", "onboard-actions");
    actions.append(yes, no);
    box.append(
      el("p", "restore-q", `Are you sure you want to delete the backup from ${b.created}?`),
      el("p", "hint", "This only removes Prism's copy on this computer; nothing on your devices changes. It can't be undone, so you won't be able to restore from it afterwards."),
      actions,
    );
    row.after(box);
    yes.focus();
  }

  async function renderList() {
    const backups = await invoke("list_backups");
    $("backup-list").replaceChildren(...(backups.length ? backups.map((b) => {
      const row = el("div", "layer-row backup-row");
      const text = el("span", "layer-text");
      const meta = `${summary(b.keyboard, "Keyboard")} · ${summary(b.mouse, "Mouse")}`;
      text.append(el("span", "layer-name", b.created), el("span", "layer-meta", b.note ? `${b.note} · ${meta}` : meta));
      const restore = el("button", "mini restore-btn", "Restore…");
      restore.addEventListener("click", () => askRestore(b, row));
      const del = el("button", "mini danger", "Delete…");
      del.addEventListener("click", () => askDelete(b, row));
      const buttons = el("div", "backup-buttons");
      buttons.append(restore, del);
      row.append(text, buttons);
      return row;
    }) : [el("p", "hint", "No backups yet. Press Back up now to make your first one.")]));
  }

  async function backupNow() {
    if (busy) return;
    busy = true;
    $("backup-now").disabled = true;
    $("backup-status").textContent = "Reading your keyboard and mouse…";
    try {
      const m = await invoke("backup_onboard");
      const parts = [summary(m.keyboard, "Keyboard"), summary(m.mouse, "Mouse")];
      $("backup-status").textContent = `Backed up. ${parts.join(" · ")}`;
    } catch (e) {
      $("backup-status").textContent = `Couldn't back up: ${e}`;
    }
    busy = false;
    $("backup-now").disabled = false;
    renderList();
  }

  /* ---------- save to device ---------- */

  let kind = "mouse"; // which device's Save section is showing
  const NAMES = { mouse: "mouse", keyboard: "keyboard" };

  function showSave() {
    $("save-title").textContent = `Save to ${NAMES[kind]}`;
    $("save-text").innerHTML = kind === "mouse"
      ? "Puts the active profile's <b>button bindings</b> and <b>DPI</b> into one of the mouse's own profiles, so they work when Prism isn't running, even on another computer. Prism shows what will be saved and backs up the mouse first."
      : "Puts the active profile's <b>key bindings</b> into one of the keyboard's own profiles, so they work when Prism isn't running, even on another computer. Prism shows what will be saved and backs up the keyboard first.";
    $("save-check").textContent = "Check what would be saved";
    $("save-status").textContent = "";
    $("save-plan").replaceChildren();
  }

  async function checkSave() {
    const box = $("save-plan");
    const dev = NAMES[kind];
    $("save-check").disabled = true;
    $("save-status").textContent = `Reading the ${dev}'s profiles…`;
    box.replaceChildren();
    let plan;
    try {
      plan = await invoke(kind === "mouse" ? "mouse_save_plan" : "kb_save_plan");
    } catch (e) {
      $("save-status").textContent = `Couldn't check: ${e}`;
      $("save-check").disabled = false;
      return;
    }
    $("save-status").textContent = "";
    $("save-check").disabled = false;
    if (!plan.profiles.length) {
      box.append(el("p", "hint", `The ${dev} didn't list any profiles, so there's nowhere to save.`));
      return;
    }

    const name = Lighting.profile().name;
    const lines = el("ul", "restore-lines");
    if (plan.saved.length) plan.saved.forEach(([b, k]) => lines.append(el("li", null, `${b} → ${k}`)));
    else lines.append(el("li", null, kind === "mouse" ? "No button bindings (buttons will do what they normally do)" : "No key bindings (keys will type what they normally do)"));
    if (kind === "mouse") {
      if (plan.dpi) {
        const [sniper, [s1, s2, s3]] = plan.dpi;
        lines.append(el("li", null, `DPI stages ${s1} / ${s2} / ${s3}, sniper ${sniper}`));
      } else {
        lines.append(el("li", null, "DPI: left as it is (DPI isn't turned on in this profile)"));
      }
    }
    const skipped = el("ul", "restore-lines");
    plan.skipped.forEach(([b, why]) => skipped.append(el("li", null, `${b}: ${why}`)));

    const id = (p) => (kind === "mouse" ? p.folder : p.root);
    const pick = el("div", "seg");
    pick.style.gridTemplateColumns = `repeat(${Math.min(plan.profiles.length, 4)}, 1fr)`;
    let target = id(plan.profiles[0]);
    plan.profiles.forEach((p) => {
      const b = el("button", null, p.name);
      b.title = kind === "mouse" ? `Holds now: DPI ${p.stages.join(" / ")}, sniper ${p.sniper}` : `Holds now: ${count(p.bound, "bound key")}`;
      b.setAttribute("aria-pressed", String(id(p) === target));
      b.addEventListener("click", () => {
        target = id(p);
        pick.querySelectorAll("button").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
      });
      pick.append(b);
    });

    // Lighting: a still picture of how the device looks right now.
    const light = el("label", "switch");
    const lightBox = el("input");
    lightBox.type = "checkbox";
    lightBox.checked = true;
    const track = el("span", "switch-track");
    track.append(el("span", "switch-thumb"));
    light.append(lightBox, track, el("span", "switch-text", "Also save lighting"));
    const lightHint = el("p", "hint", kind === "mouse"
      ? "Rainbow, Color cycle and Breathe keep moving, using the mouse's own effects (at its normal speed). Everything else is saved as it looks right now. The DPI light keeps showing the DPI stage."
      : "Rainbow, Color cycle and Breathe keep moving, using the keyboard's own effects (at its normal speed). Everything else is saved as it looks right now, as a still picture.");

    const save = el("button", "btn", `Save to ${dev}`);
    const cancel = el("button", "mini", "Cancel");
    cancel.addEventListener("click", () => box.replaceChildren());
    save.addEventListener("click", async () => {
      save.disabled = cancel.disabled = true;
      pick.querySelectorAll("button").forEach((x) => (x.disabled = true));
      $("save-status").textContent = `Backing up the ${dev}, then saving…`;
      try {
        const lighting = lightBox.checked;
        lightBox.disabled = true;
        const r = kind === "mouse" ? await invoke("mouse_save", { folder: target, lighting }) : await invoke("kb_save", { root: target, lighting });
        const parts = kind === "mouse"
          ? [count(r.buttons, "button binding"), ...(r.dpi ? ["DPI"] : []), ...(r.lighting ? ["lighting"] : [])]
          : [count(r.keys, "key binding"), ...(r.layers != null ? [`lighting (${count(r.layers, "layer")})`] : [])];
        const what = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0];
        box.replaceChildren(el("p", "restore-q", `Saved to “${r.profile}”: ${what}, and checked by reading it back. A backup (${r.backup}) was made first, so this can be undone.`));
        $("save-status").textContent = "";
      } catch (e) {
        $("save-status").textContent = `Couldn't save: ${e}`;
        save.disabled = cancel.disabled = false;
        lightBox.disabled = false;
      }
      renderList();
    });

    box.append(
      el("p", "restore-q", `From your Prism profile “${name}”, this will be saved:`),
      lines,
      ...(plan.skipped.length ? [el("p", "hint", "These won't be saved (they still work while Prism is running):"), skipped] : []),
      el("p", "restore-q", `Into which of the ${dev}'s profiles?`),
      pick,
      light,
      lightHint,
      el("p", "hint", kind === "mouse"
        ? "This replaces that profile's button bindings (and DPI, if shown above), and its lighting if switched on."
        : "This replaces that profile's key bindings, and its lighting if switched on."),
      el("div", "onboard-actions"),
    );
    box.lastChild.append(save, cancel);
  }

  function init() {
    $("save-check").addEventListener("click", checkSave);
    $("backup-now").addEventListener("click", backupNow);
    $("backup-open").addEventListener("click", () => invoke("open_backups").catch((e) => {
      $("backup-status").textContent = `Couldn't open the folder: ${e}`;
    }));
  }

  // `which` is "keyboard" or "mouse": the device whose Save section shows.
  function show(which) {
    if (which && which !== kind) kind = which;
    showSave();
    renderList();
  }

  return { init, show };
})();
