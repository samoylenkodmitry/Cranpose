using System.Runtime.InteropServices.JavaScript;
using Avalonia;
using Avalonia.Browser;
using Avalonia.Platform;

namespace PerfAvalonia;

/// <summary>The gauntlet in a page, launched by its query string: `?tier=12&amp;freeze=120`.</summary>
static class Program
{
    static async Task Main()
    {
        var query = JSHost.GlobalThis.GetPropertyAsJSObject("location")?.GetPropertyAsString("search") ?? "";
        Launch.Tier = Value(query, "tier", 5);
        Launch.Freeze = Value(query, "freeze", 0);
        Launch.Log = Console.WriteLine;
        Launch.ReadFont = file =>
        {
            using var stream = AssetLoader.Open(new Uri($"avares://PerfAvalonia/Fonts/{file}"));
            using var bytes = new MemoryStream();
            stream.CopyTo(bytes);
            return bytes.ToArray();
        };
        await AppBuilder.Configure<App>().StartBrowserAppAsync("out");
    }

    static int Value(string query, string name, int fallback)
    {
        foreach (var pair in query.TrimStart('?').Split('&'))
        {
            var parts = pair.Split('=');
            if (parts.Length == 2 && parts[0] == name && int.TryParse(parts[1], out var value)) return value;
        }
        return fallback;
    }
}
