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

  async function renderList() {
    const backups = await invoke("list_backups");
    $("backup-list").replaceChildren(...(backups.length ? backups.map((b) => {
      const row = el("div", "layer-row backup-row");
      const text = el("span", "layer-text");
      text.append(el("span", "layer-name", b.created), el("span", "layer-meta", `${summary(b.keyboard, "Keyboard")} · ${summary(b.mouse, "Mouse")}`));
      row.append(text);
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

  function init() {
    $("backup-now").addEventListener("click", backupNow);
    $("backup-open").addEventListener("click", () => invoke("open_backups").catch((e) => {
      $("backup-status").textContent = `Couldn't open the folder: ${e}`;
    }));
  }

  return { init, show: renderList };
})();
