using System;
using HarmonyLib;
using Photon.Pun;
using Photon.Realtime;
using Photon.Voice.Unity;

namespace FluxRec.Plugin.Patches;

/// <summary>
/// Injects the Flux Rec Photon App IDs so multiplayer and voice connect
/// to our Photon applications instead of Rec Room's.
/// </summary>
public static class PhotonPatches
{
    // Flux Rec Photon applications (provided by the project owner).
    public const string PunAppId   = "ec2eaafc-0c8d-4e68-8f5f-fe4b1d3fb02f";
    public const string VoiceAppId = "1619182a-c5b2-4bae-8ce0-cad44ccb30c7";

    private static bool _injected;

    private static void Inject(AppSettings settings, string source)
    {
        if (settings == null) return;
        settings.AppIdRealtime = PunAppId;
        settings.AppIdVoice = VoiceAppId;
        if (!_injected)
        {
            _injected = true;
            FluxLog.Info($"Photon App IDs injected ({source}).");
        }
    }

    // PUN reads its configuration from the PhotonServerSettings ScriptableObject
    // asset. Intercept the load and swap in our App IDs before the game uses it.
    [HarmonyPatch(typeof(UnityEngine.Resources), "Load", new Type[] { typeof(string), typeof(Type) })]
    [HarmonyPostfix]
    public static void Resources_Load_Postfix(ref UnityEngine.Object __result)
    {
        if (__result is ServerSettings serverSettings)
            Inject(serverSettings.AppSettings, "ServerSettings asset");
    }

    // Photon Voice takes its AppSettings as a parameter at connect time.
    // Cover the base virtual so any override benefits as well.
    [HarmonyPatch(typeof(VoiceConnection), "ConnectUsingSettings")]
    [HarmonyPrefix]
    public static void VoiceConnection_ConnectUsingSettings_Prefix(AppSettings __0)
    {
        Inject(__0, "VoiceConnection.ConnectUsingSettings");
    }
}
