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

  async function renderList() {
    const backups = await invoke("list_backups");
    $("backup-list").replaceChildren(...(backups.length ? backups.map((b) => {
      const row = el("div", "layer-row backup-row");
      const text = el("span", "layer-text");
      const meta = `${summary(b.keyboard, "Keyboard")} · ${summary(b.mouse, "Mouse")}`;
      text.append(el("span", "layer-name", b.created), el("span", "layer-meta", b.note ? `${b.note} · ${meta}` : meta));
      const restore = el("button", "mini restore-btn", "Restore…");
      restore.addEventListener("click", () => askRestore(b, row));
      row.append(text, restore);
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

  /* ---------- save to mouse ---------- */

  async function checkMouse() {
    const box = $("mouse-plan");
    $("mouse-check").disabled = true;
    $("mouse-status").textContent = "Reading the mouse's profiles…";
    box.replaceChildren();
    let plan;
    try {
      plan = await invoke("mouse_save_plan");
    } catch (e) {
      $("mouse-status").textContent = `Couldn't check: ${e}`;
      $("mouse-check").disabled = false;
      return;
    }
    $("mouse-status").textContent = "";
    $("mouse-check").disabled = false;
    if (!plan.profiles.length) {
      box.append(el("p", "hint", "The mouse didn't list any profiles, so there's nowhere to save."));
      return;
    }

    const name = Lighting.profile().name;
    const lines = el("ul", "restore-lines");
    if (plan.saved.length) plan.saved.forEach(([b, k]) => lines.append(el("li", null, `${b} → ${k}`)));
    else lines.append(el("li", null, "No button keys (buttons will do what they normally do)"));
    if (plan.dpi) {
      const [sniper, [s1, s2, s3]] = plan.dpi;
      lines.append(el("li", null, `DPI stages ${s1} / ${s2} / ${s3}, sniper ${sniper}`));
    } else {
      lines.append(el("li", null, "DPI: left as it is (DPI isn't turned on in this profile)"));
    }
    const skipped = el("ul", "restore-lines");
    plan.skipped.forEach(([b, why]) => skipped.append(el("li", null, `${b}: ${why}`)));

    const pick = el("div", "seg");
    pick.style.gridTemplateColumns = `repeat(${Math.min(plan.profiles.length, 4)}, 1fr)`;
    let folder = plan.profiles[0].folder;
    plan.profiles.forEach((p) => {
      const b = el("button", null, p.name);
      b.title = `Holds now: DPI ${p.stages.join(" / ")}, sniper ${p.sniper}`;
      b.setAttribute("aria-pressed", String(p.folder === folder));
      b.addEventListener("click", () => {
        folder = p.folder;
        pick.querySelectorAll("button").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
      });
      pick.append(b);
    });

    const save = el("button", "btn", "Save to mouse");
    const cancel = el("button", "mini", "Cancel");
    cancel.addEventListener("click", () => box.replaceChildren());
    save.addEventListener("click", async () => {
      save.disabled = cancel.disabled = true;
      pick.querySelectorAll("button").forEach((x) => (x.disabled = true));
      $("mouse-status").textContent = "Backing up the mouse, then saving…";
      try {
        const r = await invoke("mouse_save", { folder });
        box.replaceChildren(el("p", "restore-q", `Saved to “${r.profile}”: ${count(r.buttons, "button key")}${r.dpi ? " and DPI" : ""}, and checked by reading it back. A backup (${r.backup}) was made first, so this can be undone.`));
        $("mouse-status").textContent = "";
      } catch (e) {
        $("mouse-status").textContent = `Couldn't save: ${e}`;
        save.disabled = cancel.disabled = false;
      }
      renderList();
    });

    box.append(
      el("p", "restore-q", `From your Prism profile “${name}”, this will be saved:`),
      lines,
      ...(plan.skipped.length ? [el("p", "hint", "These need Prism running, so they won't be saved:"), skipped] : []),
      el("p", "restore-q", "Into which of the mouse's profiles?"),
      pick,
      el("p", "hint", "This replaces that profile's button keys (and DPI, if shown above). Lighting there isn't changed."),
      el("div", "onboard-actions"),
    );
    box.lastChild.append(save, cancel);
  }

  function init() {
    $("mouse-check").addEventListener("click", checkMouse);
    $("backup-now").addEventListener("click", backupNow);
    $("backup-open").addEventListener("click", () => invoke("open_backups").catch((e) => {
      $("backup-status").textContent = `Couldn't open the folder: ${e}`;
    }));
  }

  return { init, show: renderList };
})();
