using HarmonyLib;

namespace FluxRec.Plugin.Patches;

/// <summary>
/// Lets the game run without the Steam client installed.
/// The launcher also ships the Goldberg emulator as the primary bypass;
/// these patches are the in-game fallback so a missing/unresponsive Steam
/// client can never block startup.
/// </summary>
public static class SteamPatches
{
    // If SteamAPI.Init() reports failure (no Steam client), pretend it worked.
    // The game only gates startup on this boolean; it does not need real Steam
    // services because auth/multiplayer go through our own backend + Photon.
    [HarmonyPatch(typeof(Steamworks.SteamAPI), nameof(Steamworks.SteamAPI.Init), new System.Type[0])]
    [HarmonyPostfix]
    public static void SteamApi_Init_Postfix(ref bool __result)
    {
        if (!__result)
        {
            __result = true;
            FluxLog.Info("SteamAPI.Init() reported failure; bypassed (no Steam client needed).");
        }
    }

    // Belt and suspenders: the game's SteamManager gates on this property.
    [HarmonyPatch(typeof(SteamManager), "get_Initialized")]
    [HarmonyPostfix]
    public static void SteamManager_Initialized_Postfix(ref bool __result)
    {
        if (!__result)
        {
            __result = true;
            FluxLog.Info("SteamManager.Initialized forced to true.");
        }
    }
}
