//! Flux Rec — simple CLI launcher (fallback build).
//!
//! No Tauri, no Node.js, no web UI. Pure Rust, console output.
//!
//! What it does, in order:
//!   1. Resolve the game dir (`--game-dir`, `FLUXREC_GAME_DIR`, `<exe>/game`, or `<exe>` itself).
//!   2. Check game files (`RecRoom.exe`); download+extract a zip if `FLUXREC_GAME_URL` is set.
//!   3. Ensure BepInEx 6.0.0-pre.2 (markers check, download upstream zip if missing).
//!   4. Sync `RecNetPlugin.dll` from the HF mirror via the `.sha256` sidecar (fail-soft).
//!   5. Ping the backend (fail-soft, informational only).
//!   6. Launch `RecRoom.exe`, passing through any extra CLI args.
//!
//! Usage:
//!   FluxRecLauncher [--game-dir PATH] [--check-only] [--no-pause] [--play] [game args...]

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Constants (mirrored from launcher/core/src/lib.rs)
// ---------------------------------------------------------------------------

const HF_MIRROR: &str = "https://huggingface.co/datasets/Echoxr/rrflux-game";
const PLUGIN_DLL_URL: &str =
    "https://huggingface.co/datasets/Echoxr/rrflux-game/resolve/main/RecNetPlugin.dll";
const PLUGIN_SIDECAR_URL: &str =
    "https://huggingface.co/datasets/Echoxr/rrflux-game/resolve/main/RecNetPlugin.dll.sha256";
const BEPINEX_URL: &str = "https://github.com/BepInEx/BepInEx/releases/download/v6.0.0-pre.2/BepInEx-Unity.IL2CPP-win-x64-6.0.0-pre.2.zip";
const ECON_HEALTH_URL: &str = "https://fluxrec-econ.ripo-ripoteam.workers.dev/health";

const GAME_EXE: &str = "RecRoom.exe";
const PLUGIN_REL: &str = "BepInEx/plugins/RecNetPlugin.dll";
const FOSSIL_PLUGIN_REL: &str = "BepInEx/plugins/FluxRec.Plugin.dll";

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    println!(
        "=== Flux Rec Launcher (simple) v{} ===",
        env!("CARGO_PKG_VERSION")
    );

    let args: Vec<String> = env::args().skip(1).collect();
    let mut game_dir_arg: Option<String> = None;
    let mut check_only = false;
    let mut no_pause = env::var("FLUXREC_NO_PAUSE").is_ok();
    let mut passthrough: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--game-dir" => {
                i += 1;
                if i < args.len() {
                    game_dir_arg = Some(args[i].clone());
                } else {
                    eprintln!("[!] --game-dir needs a path");
                    pause_exit(1, no_pause);
                }
            }
            "--check-only" | "--verify" => check_only = true,
            "--no-pause" => no_pause = true,
            "--play" => {} // accepted for compatibility with the old launcher line
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => passthrough.push(other.to_string()),
        }
        i += 1;
    }

    let game_dir = resolve_game_dir(game_dir_arg);
    println!("[*] Game dir: {}", game_dir.display());
    let _ = HF_MIRROR; // documented for operators; mirror URLs are baked into the constants above

    // 1. Game files
    if let Err(e) = ensure_game_files(&game_dir) {
        eprintln!("[!] Game files: {e}");
        pause_exit(1, no_pause);
    }
    println!("[ok] Game files present ({GAME_EXE})");

    // 2. BepInEx
    match ensure_bepinex(&game_dir) {
        Ok(true) => println!("[ok] BepInEx installed (first run)"),
        Ok(false) => println!("[ok] BepInEx already present"),
        Err(e) => {
            eprintln!("[!] BepInEx: {e}");
            pause_exit(1, no_pause);
        }
    }

    // 3. Plugin sync (fail-soft: never strand the player)
    match sync_plugin(&game_dir) {
        Ok(detail) => println!("[ok] Plugin: {detail}"),
        Err(e) => println!("[warn] Plugin sync failed, keeping installed DLL: {e}"),
    }

    // 4. Backend ping (informational only)
    backend_ping();

    if check_only {
        println!("[*] Check complete (--check-only), not launching.");
        return;
    }

    // 5. Launch
    match launch_game(&game_dir, &passthrough) {
        Ok(code) => println!("[*] Game exited with code {code}"),
        Err(e) => {
            eprintln!("[!] Launch failed: {e}");
            pause_exit(1, no_pause);
        }
    }
}

fn print_help() {
    println!(
        "Flux Rec simple launcher\n\
         \n\
         Usage: FluxRecLauncher [options] [game args...]\n\
         \n\
         Options:\n\
           --game-dir PATH   Use PATH as the game directory\n\
           --check-only      Verify install, do not launch the game\n\
           --no-pause        Never wait for Enter on error (for scripts/CI)\n\
           --play            Accepted for compatibility (no-op)\n\
           --help            This text\n\
         \n\
         Env vars:\n\
           FLUXREC_GAME_DIR  Game directory override\n\
           FLUXREC_GAME_URL  Zip URL to download when RecRoom.exe is missing\n\
           FLUXREC_NO_PAUSE  Same as --no-pause"
    );
}

// ---------------------------------------------------------------------------
// Game dir resolution
// ---------------------------------------------------------------------------

fn resolve_game_dir(arg: Option<String>) -> PathBuf {
    if let Some(a) = arg {
        return PathBuf::from(a);
    }
    if let Ok(e) = env::var("FLUXREC_GAME_DIR") {
        if !e.trim().is_empty() {
            return PathBuf::from(e);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Launcher sitting next to the game install.
            if dir.join(GAME_EXE).is_file() {
                return dir.to_path_buf();
            }
            // Otherwise expect a "game" subfolder next to the launcher.
            return dir.join("game");
        }
    }
    PathBuf::from("game")
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent("FluxRecLauncherSimple/1.0")
        .build()
        .expect("http client")
}

/// Stream-download a URL to `dest` (via a `.part` file, then atomic rename).
fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let mut resp = http_client()
        .get(url)
        .send()
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} for {url}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = dest.with_extension("part");
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut done: u64 = 0;
    let mut last_print: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("download read failed: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if done - last_print >= 10 * 1024 * 1024 {
            last_print = done;
            if total > 0 {
                println!(
                    "    ... {:.1} / {:.1} MB",
                    done as f64 / 1_000_000.0,
                    total as f64 / 1_000_000.0
                );
            } else {
                println!("    ... {:.1} MB", done as f64 / 1_000_000.0);
            }
        }
    }
    drop(f);
    fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Extract a zip with zip-slip protection (entries that escape `dest` are skipped).
fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let f = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("bad zip: {e}"))?;
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let out = match entry.enclosed_name() {
            Some(p) => dest.join(p),
            None => continue,
        };
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut outf = fs::File::create(&out).map_err(|e| e.to_string())?;
            io::copy(&mut entry, &mut outf).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Step 1: game files
// ---------------------------------------------------------------------------

fn ensure_game_files(game_dir: &Path) -> Result<(), String> {
    if game_dir.join(GAME_EXE).is_file() {
        return Ok(());
    }
    let url = env::var("FLUXREC_GAME_URL").unwrap_or_default();
    if url.trim().is_empty() {
        return Err(format!(
            "{GAME_EXE} not found in {} and FLUXREC_GAME_URL is not set.\n\
             Put this launcher next to your game folder (or in a 'game' subfolder),\n\
             or set FLUXREC_GAME_URL to a zip containing the game files.",
            game_dir.display()
        ));
    }
    println!("[*] Game files missing — downloading...");
    let tmp = env::temp_dir().join("fluxrec-game.zip");
    download_file(url.trim(), &tmp)?;
    println!("[*] Extracting game files (this takes a while)...");
    extract_zip(&tmp, game_dir)?;
    let _ = fs::remove_file(&tmp);
    if !game_dir.join(GAME_EXE).is_file() {
        return Err("downloaded archive did not contain RecRoom.exe".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Step 2: BepInEx
// ---------------------------------------------------------------------------

fn bepinex_markers(game_dir: &Path) -> Vec<PathBuf> {
    [
        "BepInEx/core/BepInEx.Core.dll",
        "BepInEx/core/BepInEx.Preloader.dll",
        "doorstop_config.ini",
        "winhttp.dll",
    ]
    .iter()
    .map(|m| game_dir.join(m))
    .collect()
}

/// Returns Ok(true) if it installed BepInEx, Ok(false) if already present.
fn ensure_bepinex(game_dir: &Path) -> Result<bool, String> {
    if bepinex_markers(game_dir).iter().all(|p| p.is_file()) {
        return Ok(false);
    }
    println!("[*] BepInEx missing — downloading...");
    let tmp = env::temp_dir().join("bepinex-install.zip");
    download_file(BEPINEX_URL, &tmp)?;
    println!("[*] Extracting BepInEx...");
    extract_zip(&tmp, game_dir)?;
    let _ = fs::remove_file(&tmp);
    if !bepinex_markers(game_dir).iter().all(|p| p.is_file()) {
        let missing: Vec<String> = bepinex_markers(game_dir)
            .iter()
            .filter(|p| !p.is_file())
            .map(|p| p.display().to_string())
            .collect();
        return Err(format!("BepInEx install incomplete, missing: {}", missing.join(", ")));
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Step 3: plugin sync (fail-soft)
// ---------------------------------------------------------------------------

fn sync_plugin(game_dir: &Path) -> Result<String, String> {
    let sidecar = http_client()
        .get(PLUGIN_SIDECAR_URL)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .map_err(|e| format!("offline: {e}"))?;
    if !sidecar.status().is_success() {
        return Err(format!("sidecar HTTP {}", sidecar.status()));
    }
    let expected = sidecar
        .text()
        .map_err(|e| e.to_string())?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if expected.is_empty() {
        return Err("empty sidecar".into());
    }

    let dest = game_dir.join(PLUGIN_REL);
    if dest.is_file() {
        if let Ok(h) = sha256_file(&dest) {
            if h.eq_ignore_ascii_case(&expected) {
                return Ok(format!("up to date ({})", &h[..12.min(h.len())]));
            }
        }
    }

    // Remove the fossil stub from the old launcher line, if present.
    let _ = fs::remove_file(game_dir.join(FOSSIL_PLUGIN_REL));

    println!("[*] Downloading RecNetPlugin.dll...");
    let tmp = env::temp_dir().join("RecNetPlugin.dll.new");
    download_file(PLUGIN_DLL_URL, &tmp)?;
    let got = sha256_file(&tmp)?;
    if !got.eq_ignore_ascii_case(&expected) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("hash mismatch (got {got}, want {expected})"));
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(format!("updated to {}", &expected[..12.min(expected.len())]))
}

// ---------------------------------------------------------------------------
// Step 4: backend ping (informational only)
// ---------------------------------------------------------------------------

fn backend_ping() {
    let ok = http_client()
        .get(ECON_HEALTH_URL)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    if ok {
        println!("[ok] Backend reachable");
    } else {
        println!("[warn] Backend unreachable (continuing anyway)");
    }
}

// ---------------------------------------------------------------------------
// Step 5: launch
// ---------------------------------------------------------------------------

fn launch_game(game_dir: &Path, extra_args: &[String]) -> Result<i32, String> {
    let exe = game_dir.join(GAME_EXE);
    if !exe.is_file() {
        return Err(format!("{} not found", exe.display()));
    }
    println!("[*] Launching {} ...", exe.display());
    let mut child = Command::new(&exe)
        .current_dir(game_dir)
        .args(extra_args)
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;
    println!("[*] Game running (pid {}), waiting for exit...", child.id());
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(-1))
}

// ---------------------------------------------------------------------------
// Exit helper
// ---------------------------------------------------------------------------

/// On fatal errors, wait for Enter so a double-clicked console window doesn't
/// vanish before the user can read the message. Skipped when stdin isn't a
/// terminal (CI/scripts) or --no-pause / FLUXREC_NO_PAUSE is set.
fn pause_exit(code: i32, no_pause: bool) -> ! {
    if !no_pause && io::stdin().is_terminal() {
        println!("Press Enter to exit...");
        let mut s = String::new();
        let _ = io::stdin().read_line(&mut s);
    }
    std::process::exit(code);
}
