using Avalonia;

namespace PerfAvalonia;

/// <summary>The gauntlet in a desktop window, as `desktop.py` runs it.</summary>
static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        Launch.Tier = Variable("PERF_TIER", 5);
        Launch.Freeze = Variable("PERF_FREEZE", 0);
        Launch.Log = Console.WriteLine;
        AppBuilder.Configure<App>().UsePlatformDetect().StartWithClassicDesktopLifetime(args);
    }

    static int Variable(string name, int fallback) =>
        int.TryParse(Environment.GetEnvironmentVariable(name), out var value) ? value : fallback;
}
