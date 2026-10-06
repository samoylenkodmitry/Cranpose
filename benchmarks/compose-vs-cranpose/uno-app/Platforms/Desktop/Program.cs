using Uno.UI.Hosting;

namespace PerfUno;

/// <summary>The gauntlet in a desktop window, as `desktop.py` runs it.</summary>
internal static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        Launch.Tier = Variable("PERF_TIER", 5);
        Launch.Freeze = Variable("PERF_FREEZE", 0);
        Launch.Log = Console.WriteLine;
        UnoPlatformHostBuilder.Create()
            .App(() => new App())
            .UseMacOS()
            .UseX11()
            .UseWin32()
            .Build()
            .Run();
    }

    static int Variable(string name, int fallback) =>
        int.TryParse(Environment.GetEnvironmentVariable(name), out var value) ? value : fallback;
}
