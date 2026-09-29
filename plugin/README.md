# FluxRec.Plugin (v2)

BepInEx 6.0.0-pre.2 plugin for the Flux Rec client (Rec Room 20230414, Unity 2020.3.33 IL2CPP).

- **GUID:** `gg.ripoteam.fluxrec`
- **Name:** `FluxRec.Plugin`
- **Version:** `2.0.0`

## What it does

1. **API redirect** (`Patches/HttpPatches.cs`, `UrlRedirect.cs`)
   Rewrites exact RecNet hosts to the Flux Rec v2 Cloudflare workers, preserving
   path/query. Covers both HTTP stacks the client uses (BestHTTP and
   UnityWebRequest). Unknown hosts pass through untouched.
   - `api.rec.net`, `www.rec.net`, `rec.net` → `fluxrec-api.ripo-ripoteam.workers.dev`
   - `auth.rec.net` → `fluxrec-auth.ripo-ripoteam.workers.dev`
   - `econ.rec.net` → `fluxrec-econ.ripo-ripoteam.workers.dev`
   - `img.rec.net`, `cdn.rec.net` → `fluxrec-api.ripo-ripoteam.workers.dev`
2. **Steam bypass** (`Patches/SteamPatches.cs`)
   `SteamAPI.Init()` failure is treated as success and
   `SteamManager.Initialized` is forced true, so the game starts with no Steam
   client installed. (The launcher also ships the Goldberg emulator.)
3. **Photon App IDs** (`Patches/PhotonPatches.cs`)
   Injects the Flux Rec Photon Realtime and Voice App IDs into PUN's
   `ServerSettings` asset at load and into Photon Voice at connect time.
4. **Logging** (`FluxLog.cs`)
   Writes `FluxRec.log` next to the game executable. Redirects are sampled
   (1 in 50) so the log stays readable; lifecycle events are always logged.

## Build

```sh
cd ~/workspace/fluxrec-v2/plugin
dotnet build -c Release
```

Output: `bin/Release/netstandard2.1/FluxRec.Plugin.dll`

References resolve against the v2 client tree (`../client/...`) and the
Il2CppDumper stubs; they are compile-time only (`Private=false`).

## Install

Copy `FluxRec.Plugin.dll` to `<game>/BepInEx/plugins/`.
