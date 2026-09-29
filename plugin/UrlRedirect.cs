using System;
using System.Collections.Generic;

namespace FluxRec.Plugin;

/// <summary>
/// Maps Rec Room's production API hosts to the Flux Rec v2 backend workers.
/// Matching is exact-host only (never prefix/substring) so unrelated URLs
/// are never touched. Scheme, path, query and fragment are preserved.
/// </summary>
public static class UrlRedirect
{
    public const string ApiWorker  = "fluxrec-api.ripo-ripoteam.workers.dev";
    public const string AuthWorker = "fluxrec-auth.ripo-ripoteam.workers.dev";
    public const string EconWorker = "fluxrec-econ.ripo-ripoteam.workers.dev";

    private static readonly Dictionary<string, string> HostMap =
        new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            // Main game API surface
            ["api.rec.net"]  = ApiWorker,
            ["www.rec.net"]  = ApiWorker,
            ["rec.net"]      = ApiWorker,
            // Auth
            ["auth.rec.net"] = AuthWorker,
            // Economy / storefronts
            ["econ.rec.net"] = EconWorker,
            // Static assets served through the API worker
            ["img.rec.net"]  = ApiWorker,
            ["cdn.rec.net"]  = ApiWorker,
        };

    /// <summary>
    /// Returns true and the rewritten URL when the host is a known RecNet host.
    /// </summary>
    public static bool TryRewrite(string url, out string rewritten)
    {
        rewritten = null;
        if (string.IsNullOrEmpty(url))
            return false;

        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri))
            return false;

        if (!HostMap.TryGetValue(uri.Host, out var newHost))
            return false;

        var builder = new UriBuilder(uri) { Host = newHost };
        // Keep the original port only if it was explicit and non-default.
        if (uri.IsDefaultPort)
            builder.Port = -1;

        rewritten = builder.Uri.ToString();
        FluxLog.Redirect(url, rewritten);
        return true;
    }

    /// <summary>
    /// Uri overload for APIs that pass System.Uri directly (e.g. BestHTTP).
    /// </summary>
    public static bool TryRewrite(Uri uri, out Uri rewritten)
    {
        rewritten = null;
        if (uri == null)
            return false;

        if (!HostMap.TryGetValue(uri.Host, out var newHost))
            return false;

        var builder = new UriBuilder(uri) { Host = newHost };
        if (uri.IsDefaultPort)
            builder.Port = -1;

        rewritten = builder.Uri;
        FluxLog.Redirect(uri.ToString(), rewritten.ToString());
        return true;
    }
}
