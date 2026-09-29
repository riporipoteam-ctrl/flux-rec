using HarmonyLib;

namespace FluxRec.Plugin.Patches;

/// <summary>
/// Forces the new RRUI Watch UI (new home screen with Play button, hot rooms, working Store).
/// The old Watch UI's Store page crashes during UI init (zero network calls) — it's not a backend problem.
/// The new RRUI home screen has Play/hot-rooms natively and its Store uses Storefront_Watch (which our backend serves).
/// Without this patch, Statsig gates default to false (no key) → old UI shows → Store crashes.
/// </summary>
[HarmonyPatch]
public static class WatchUIPatches
{
    // WatchUI.get_UseRRUIHomeScreen() — force true to enable the new RRUI home screen
    [HarmonyPatch("RecRoom.Core.WatchUI", "get_UseRRUIHomeScreen")]
    [HarmonyPrefix]
    public static bool ForceRRUIHomeScreen(ref bool __result)
    {
        __result = true;
        return false; // Skip original, use our forced value
    }

    // Also force the RRUI backpack screen if it exists
    [HarmonyPatch("RecRoom.Core.WatchUI", "get_UseRRUIBackpackScreen")]
    [HarmonyPrefix]
    public static bool ForceRRUIBackpackScreen(ref bool __result)
    {
        __result = true;
        return false;
    }

    // Statsig gate check — force all RRUI gates to true
    // StatsigModel.IsFeatureGateActive(string) @ 0x1A3D4C0
    [HarmonyPatch("RecRoom.Core.StatsigModel", "IsFeatureGateActive")]
    [HarmonyPrefix]
    public static bool ForceStatsigGates(string gateName, ref bool __result)
    {
        // Only force RRUI-related gates, let others use default behavior
        if (gateName != null && gateName.StartsWith("RRUI."))
        {
            __result = true;
            return false;
        }
        return true; // Let original run for non-RRUI gates
    }
}
