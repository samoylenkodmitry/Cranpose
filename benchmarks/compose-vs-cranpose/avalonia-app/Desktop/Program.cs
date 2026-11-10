using Avalonia;

namespace PerfAvalonia;

/// <summary>The gauntlet, the still wall or the patchwork in a desktop window, as `desktop.py` runs it.</summary>
static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        Launch.Tier = Variable("PERF_TIER", 5);
        Launch.Freeze = Variable("PERF_FREEZE", 0);
        var scenario = Environment.GetEnvironmentVariable("PERF_SCENARIO");
        Launch.Wall = scenario == "wall";
        if (scenario == "patchwork") Launch.Patchwork = () => new PatchworkView(Launch.Tier, Launch.Freeze);
        Launch.Log = Console.WriteLine;
        AppBuilder.Configure<App>().UsePlatformDetect().StartWithClassicDesktopLifetime(args);
    }

    static int Variable(string name, int fallback) =>
        int.TryParse(Environment.GetEnvironmentVariable(name), out var value) ? value : fallback;
}
