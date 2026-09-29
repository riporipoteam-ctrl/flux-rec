using System;
using HarmonyLib;

namespace FluxRec.Plugin.Patches;

/// <summary>
/// Redirects the game's HTTP traffic from RecNet hosts to the Flux Rec backend.
/// Covers both HTTP stacks the client uses:
///   - BestHTTP (BestHTTP.HTTPRequest) — the game's primary API client
///   - UnityWebRequest (UnityEngine.Networking) — used for misc downloads
/// Only exact known hosts are rewritten; everything else passes through untouched.
/// </summary>
public static class HttpPatches
{
    // ---- BestHTTP -------------------------------------------------------

    [HarmonyPatch(typeof(BestHTTP.HTTPRequest), "set_Uri")]
    [HarmonyPrefix]
    public static void BestHttp_SetUri_Prefix(ref Uri value)
    {
        if (UrlRedirect.TryRewrite(value, out var rewritten))
            value = rewritten;
    }

    // ---- UnityWebRequest ------------------------------------------------

    [HarmonyPatch(typeof(UnityEngine.Networking.UnityWebRequest), "set_url")]
    [HarmonyPrefix]
    public static void Uwr_SetUrl_Prefix(ref string value)
    {
        if (UrlRedirect.TryRewrite(value, out var rewritten))
            value = rewritten;
    }

    [HarmonyPatch(typeof(UnityEngine.Networking.UnityWebRequest), "set_uri")]
    [HarmonyPrefix]
    public static void Uwr_SetUri_Prefix(ref Uri value)
    {
        if (UrlRedirect.TryRewrite(value, out var rewritten))
            value = rewritten;
    }
}
