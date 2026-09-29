/* Flux Rec Launcher frontend — vanilla JS. Talks to Rust via Tauri invoke. */
const invoke = window.__TAURI__ ? window.__TAURI__.core.invoke : null;
const listen = window.__TAURI__ ? window.__TAURI__.event.listen : null;

async function call(cmd, args) {
  if (!invoke) { console.log("[stub]", cmd, args); return null; }
  return await invoke(cmd, args || {});
}

const $ = (id) => document.getElementById(id);
function log(el, msg) { const d = document.createElement("div"); d.textContent = msg; $(el).prepend(d); }

/* ---------- screen router ---------- */
document.querySelectorAll(".nav-btn[data-screen]").forEach(b => {
  b.addEventListener("click", () => show(b.dataset.screen));
});
function show(name) {
  document.querySelectorAll(".nav-btn[data-screen]").forEach(b => b.classList.toggle("active", b.dataset.screen === name));
  document.querySelectorAll(".screen").forEach(s => s.classList.toggle("active", s.id === "screen-" + name));
}

/* ---------- progress events ---------- */
if (listen) {
  listen("launcher://progress", (e) => {
    const p = e.payload;
    const pct = p.file_total > 0 ? (p.file_done / p.file_total * 100) : 0;
    const target = p.channel === "verify" ? "verify-bar" : (wizardOpen ? "wiz-bar" : "hero-bar");
    const wrap = p.channel === "verify" ? "verify-progress" : (wizardOpen ? "wiz-progress" : "hero-progress");
    $(wrap).classList.remove("hidden");
    $(target).style.width = Math.min(100, pct).toFixed(1) + "%";
    $("hero-sub").textContent = p.message;
  });
}

/* ---------- home ---------- */
let pendingUpdates = [];
async function refreshStatus() {
  $("hero-status").textContent = "Checking…";
  try {
    const s = await call("get_status");
    if (!s) return;
    setPill("pill-game", s.game_files_ok, s.game_detail, "verify");
    setPill("pill-bepinex", s.bepinex_ok, s.bepinex_detail, "verify");
    setPill("pill-plugin", s.plugin_ok, s.plugin_detail, "verify");
    setPill("pill-backend", s.backend_ok, s.backend_detail, null);
    const allOk = s.game_files_ok && s.bepinex_ok && s.plugin_ok;
    $("hero-status").textContent = allOk ? "Ready to play" : "Setup needed";
    $("hero-status").style.color = allOk ? "var(--ok)" : "var(--warn)";
    $("btn-play").disabled = !s.game_files_ok;
    if (!s.game_files_ok) $("hero-sub").textContent = "Game files missing — run the first-run setup or Verify.";
  } catch (e) { $("hero-status").textContent = "Status check failed"; }
}
function setPill(id, ok, detail, jump) {
  const el = $(id);
  el.classList.toggle("ok", ok); el.classList.toggle("fail", !ok);
  el.querySelector(".pdetail").textContent = detail;
  el.onclick = (!ok && jump) ? () => show(jump) : null;
}
async function refreshChangelog() {
  try { const c = await call("get_changelog"); if (c) $("changelog").textContent = c.trim() || "No notes."; }
  catch { $("changelog").textContent = "Couldn't load release notes (offline?)."; }
}
$("btn-play").addEventListener("click", async () => {
  $("btn-play").disabled = true; $("hero-status").textContent = "Launching…";
  try {
    const actions = await call("play");
    (actions || []).forEach(a => log("home-log", a));
    $("hero-sub").textContent = "Game starting…";
  } catch (e) { $("hero-status").textContent = "Launch failed"; log("home-log", String(e)); $("btn-play").disabled = false; }
});
$("btn-skip").addEventListener("click", () => $("btn-play").click());

/* ---------- updates ---------- */
async function refreshUpdates() {
  const box = $("channels"); box.innerHTML = "<p class='muted'>Checking…</p>";
  try {
    const checks = await call("check_updates") || [];
    pendingUpdates = checks.filter(c => c.available);
    $("nav-update-dot").classList.toggle("hidden", pendingUpdates.length === 0);
    box.innerHTML = "";
    const names = { launcher: "Launcher", content: "Game files", plugin: "Plugin" };
    checks.forEach(c => {
      const d = document.createElement("div"); d.className = "channel";
      d.innerHTML = `<div class="chead"><span class="cname">${names[c.channel] || c.channel}</span>
        <span class="cver">${c.available ? `v${c.current} → v${c.latest} available` : `v${c.current} — up to date`}</span></div>
        <div class="muted">${c.detail || ""}</div>`;
      box.appendChild(d);
    });
    if (!checks.length) box.innerHTML = "<p class='muted'>Check failed (offline?). You can still play.</p>";
  } catch { box.innerHTML = "<p class='muted'>Check failed.</p>"; }
}
$("btn-update-all").addEventListener("click", async () => {
  $("btn-update-all").disabled = true;
  try {
    // Channel A first (needs launcher restart afterwards).
    const launcher = pendingUpdates.find(u => u.channel === "launcher");
    if (launcher) {
      log("updates-log", "Downloading launcher v" + launcher.latest + "…");
      const newExe = await call("fetch_launcher_update", { latest: launcher.latest, current: launcher.current });
      log("updates-log", "Launcher downloaded. Restart to apply — game files untouched.");
      // Hand off to the helper: spawn new exe with --apply-update, then exit.
      // (Shell spawn via a tiny backend command would be cleaner; for v1 we instruct.)
      alert("Launcher update downloaded. Close and re-run the new launcher to finish updating.");
    }
    const lines = await call("apply_updates") || [];
    lines.forEach(l => log("updates-log", l));
    await refreshStatus(); await refreshUpdates();
  } catch (e) { log("updates-log", "Update failed: " + e); }
  $("btn-update-all").disabled = false;
});
$("btn-play-anyway").addEventListener("click", () => { show("home"); $("btn-play").click(); });

/* ---------- verify ---------- */
let badPaths = [];
$("btn-verify").addEventListener("click", async () => {
  $("btn-verify").disabled = true; $("verify-result").innerHTML = ""; $("verify-progress").classList.remove("hidden");
  try {
    badPaths = await call("verify_files") || [];
    $("verify-progress").classList.add("hidden");
    if (!badPaths.length) { log("verify-result", "All files verified ✓"); }
    else {
      log("verify-result", badPaths.length + " file(s) differ — repairing…");
      const out = await call("repair_files", { paths: badPaths }) || [];
      out.forEach(l => log("verify-result", l));
    }
  } catch (e) { log("verify-result", "Verify failed: " + e); }
  $("btn-verify").disabled = false;
});
$("btn-repair-bepinex").addEventListener("click", async () => {
  try { log("verify-result", await call("repair_bepinex")); } catch (e) { log("verify-result", String(e)); }
});
$("btn-reset-plugin").addEventListener("click", async () => {
  try { log("verify-result", await call("reset_plugin")); } catch (e) { log("verify-result", String(e)); }
});
async function loadDefender() {
  try { const steps = await call("defender_steps") || []; $("defender-steps").innerHTML = steps.map(s => "• " + s).join("<br>"); } catch {}
}

/* ---------- settings ---------- */
async function loadSettings() {
  try {
    const s = await call("get_settings"); if (!s) return;
    $("set-dir").value = s.install_dir || "(default)";
    $("set-channel").value = s.channel;
    $("set-check").checked = s.check_updates_on_launch;
    $("set-close").checked = s.close_on_play;
    $("set-noconsole").checked = s.no_console;
  } catch {}
}
$("btn-save-settings").addEventListener("click", async () => {
  try {
    const cur = await call("get_settings");
    const dir = $("set-dir").value;
    cur.channel = $("set-channel").value;
    cur.install_dir = (dir === "(default)" ? "" : dir);
    cur.check_updates_on_launch = $("set-check").checked;
    cur.close_on_play = $("set-close").checked;
    cur.no_console = $("set-noconsole").checked;
    await call("save_settings", { s: cur });
    log("settings-log", "Settings saved.");
  } catch (e) { log("settings-log", String(e)); }
});
$("btn-browse").addEventListener("click", () => {
  const v = prompt("Install directory (leave empty for default):", $("set-dir").value === "(default)" ? "" : $("set-dir").value);
  if (v !== null) $("set-dir").value = v.trim() || "(default)";
});
$("btn-open-folder").addEventListener("click", async () => { try { await call("open_game_folder"); } catch (e) { log("settings-log", String(e)); } });
$("btn-diag").addEventListener("click", async () => {
  try {
    const d = await call("copy_diagnostics");
    await navigator.clipboard.writeText(d);
    log("settings-log", "Diagnostics copied to clipboard.");
  } catch (e) { log("settings-log", String(e)); }
});
$("btn-discord").addEventListener("click", () => window.open("https://discord.gg/ripoteam", "_blank"));
$("btn-help").addEventListener("click", () => show("verify"));

/* ---------- first-run wizard ---------- */
let wizardOpen = false, wizStep = 0;
const wizSteps = ["welcome", "directory", "download", "finalize", "done"];
async function maybeWizard() {
  try {
    const s = await call("get_settings"); if (!s) return;
    const present = await call("game_present");
    if (!s.firstrun_done && !present) openWizard();
  } catch {}
}
function openWizard() { wizardOpen = true; wizStep = 0; $("wizard").classList.remove("hidden"); renderWiz(); }
function closeWizard() { wizardOpen = false; $("wizard").classList.add("hidden"); }
function renderWiz() {
  const step = wizSteps[wizStep];
  $("wiz-back").style.visibility = wizStep === 0 ? "hidden" : "visible";
  $("wiz-next").textContent = wizStep === wizSteps.length - 1 ? "Play" : "Next";
  if (step === "welcome") {
    $("wiz-title").textContent = "Welcome to Flux Rec";
    $("wiz-body").innerHTML = "This wizard downloads the game (~4 GB), sets up BepInEx and the plugin.<br>You can pause and resume any time.";
  } else if (step === "directory") {
    $("wiz-title").textContent = "Install location";
    $("wiz-body").innerHTML = `Default is fine for most players.<br><br><input id="wiz-dir" type="text" placeholder="(default)" style="width:100%;padding:8px">`;
  } else if (step === "download") {
    $("wiz-title").textContent = "Downloading game files";
    $("wiz-body").innerHTML = "Differential download — only missing/changed files are fetched.<br>Safe to close and resume later.";
    $("wiz-next").textContent = "Start download";
  } else if (step === "finalize") {
    $("wiz-title").textContent = "Final setup";
    $("wiz-body").innerHTML = "Installing BepInEx, syncing the plugin, checking the Steam bypass…";
    $("wiz-next").textContent = "Run setup";
  } else {
    $("wiz-title").textContent = "Done";
    $("wiz-body").innerHTML = "Flux Rec is ready. Have fun!";
  }
}
$("wiz-back").addEventListener("click", () => { if (wizStep > 0) { wizStep--; renderWiz(); } });
$("wiz-next").addEventListener("click", async () => {
  const step = wizSteps[wizStep];
  if (step === "directory") {
    const v = ($("wiz-dir") || {}).value;
    if (v && v.trim()) { const s = await call("get_settings"); s.install_dir = v.trim(); await call("save_settings", { s }); }
  }
  if (step === "download") { await call("apply_updates"); }
  if (step === "finalize") { await call("play").catch(() => {}); /* self-heal only; don't launch yet */ }
  if (step === "done") {
    const s = await call("get_settings"); s.firstrun_done = true; await call("save_settings", { s });
    closeWizard(); refreshStatus(); return;
  }
  wizStep++; renderWiz();
});

/* ---------- boot ---------- */
(async function boot() {
  await loadSettings();
  await refreshStatus();
  await refreshChangelog();
  await refreshUpdates();
  await loadDefender();
  await maybeWizard();
  // Auto-check updates on launch (fail-soft).
  try {
    const s = await call("get_settings");
    if (s && s.check_updates_on_launch) await refreshUpdates();
  } catch {}
})();
