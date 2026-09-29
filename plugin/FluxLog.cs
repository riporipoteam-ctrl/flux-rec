using System;
using System.IO;
using System.Text;

namespace FluxRec.Plugin;

/// <summary>
/// Minimal file logger. Writes one line per event to FluxRec.log next to the
/// game executable. Rate-limited for hot paths so the log stays readable.
/// Not spammy by design: redirects are sampled, everything else is logged once.
/// </summary>
public static class FluxLog
{
    private static string _path;
    private static readonly object _lock = new object();

    // Sampling for the URL redirect hot path: log every Nth redirect.
    private const int RedirectSampleRate = 50;
    private static int _redirectCount;

    public static void Init()
    {
        try
        {
            var dir = AppDomain.CurrentDomain.BaseDirectory;
            _path = Path.Combine(dir, "FluxRec.log");
            File.AppendAllText(_path,
                $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss}] --- FluxRec.Plugin session start ---{Environment.NewLine}",
                Encoding.UTF8);
        }
        catch
        {
            _path = null; // logging is best-effort; never crash the game over it
        }
    }

    public static void Info(string message) => Write("INFO", message);

    public static void Warn(string message) => Write("WARN", message);

    public static void Error(string message) => Write("ERROR", message);

    /// <summary>
    /// Sampled logging for the URL redirect hot path.
    /// </summary>
    public static void Redirect(string from, string to)
    {
        var n = System.Threading.Interlocked.Increment(ref _redirectCount);
        if (n % RedirectSampleRate == 1)
            Write("REDIRECT", $"{from} -> {to} (#{n})");
    }

    private static void Write(string level, string message)
    {
        if (_path == null) return;
        try
        {
            lock (_lock)
            {
                File.AppendAllText(_path,
                    $"[{DateTime.Now:HH:mm:ss}] [{level}] {message}{Environment.NewLine}",
                    Encoding.UTF8);
            }
        }
        catch
        {
            // never throw from the logger
        }
    }
}
