using System.Reflection;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using FluxRec.Plugin.Patches;
using HarmonyLib;

namespace FluxRec.Plugin;

/// <summary>
/// Flux Rec v2 plugin entry point.
/// Redirects the game's RecNet API traffic to the Flux Rec backend,
/// bypasses the Steam client requirement, and injects our Photon App IDs.
/// </summary>
[BepInPlugin(GUID, NAME, VERSION)]
public class FluxRecPlugin : BasePlugin
{
    public const string GUID = "gg.ripoteam.fluxrec";
    public const string NAME = "FluxRec.Plugin";
    public const string VERSION = "2.0.0";

    public override void Load()
    {
        FluxLog.Init();
        FluxLog.Info($"=== {NAME} v{VERSION} loading ===");

        var harmony = new Harmony(GUID);
        int applied = 0;

        applied += ApplyPatchSet(harmony, typeof(HttpPatches), "HTTP redirect");
        applied += ApplyPatchSet(harmony, typeof(SteamPatches), "Steam bypass");
        applied += ApplyPatchSet(harmony, typeof(PhotonPatches), "Photon App IDs");

        FluxLog.Info($"=== {NAME} v{VERSION} loaded ({applied} patch groups) ===");
        Log.LogInfo($"{NAME} v{VERSION} loaded.");
    }

    private static int ApplyPatchSet(Harmony harmony, System.Type patchContainer, string label)
    {
        try
        {
            Harmony.CreateAndPatchAll(patchContainer, GUID);
            FluxLog.Info($"[OK] {label} patches applied.");
            return 1;
        }
        catch (System.Exception ex)
        {
            // One failing group must not take down the whole plugin.
            FluxLog.Error($"[FAIL] {label} patches: {ex.GetType().Name}: {ex.Message}");
            return 0;
        }
    }
}
