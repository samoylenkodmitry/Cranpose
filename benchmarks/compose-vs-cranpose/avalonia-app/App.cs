using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Media;
using Avalonia.Media.Fonts;
using Avalonia.Styling;
using Avalonia.Themes.Fluent;

namespace PerfAvalonia;

/// <summary>What `am start` asked of the gauntlet.</summary>
public static class Launch
{
    /// <summary>Load tier, 1 to 16.</summary>
    public static int Tier { get; set; } = 5;

    /// <summary>Stop on this frame and hold still, for picture comparisons; 0 runs on.</summary>
    public static int Freeze { get; set; }

    /// <summary>Writes a `PERF` line where `measure.py` reads it.</summary>
    public static Action<string> Log { get; set; } = _ => { };

    /// <summary>The bytes of a Roboto file: the device's own on Android, elsewhere the one in the
    /// folder `PERF_FONTS` names; a page reads its own copy.</summary>
    public static Func<string, byte[]> ReadFont { get; set; } = file => File.ReadAllBytes(
        Path.Combine(OperatingSystem.IsAndroid() ? "/system/fonts"
            : Environment.GetEnvironmentVariable("PERF_FONTS") ?? "fonts", file));
}

public sealed class App : Avalonia.Application
{
    /// <summary>The window every desktop app opens for the gauntlet, in logical points.</summary>
    const double DesktopWidth = 1280;
    const double DesktopHeight = 820;

    public override void Initialize()
    {
        RequestedThemeVariant = ThemeVariant.Light;
        Styles.Add(new FluentTheme());
        FontManager.Current.AddFontCollection(new DeviceRoboto());
    }

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IActivityApplicationLifetime activity)
        {
            activity.MainViewFactory = () => new GauntletView(Launch.Tier, Launch.Freeze);
        }
        else if (ApplicationLifetime is ISingleViewApplicationLifetime page)
        {
            page.MainView = new GauntletView(Launch.Tier, Launch.Freeze);
        }
        else if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            desktop.MainWindow = new Window
            {
                Title = "Gauntlet",
                Width = DesktopWidth,
                Height = DesktopHeight,
                CanResize = false,
                Content = new GauntletView(Launch.Tier, Launch.Freeze),
            };
        }
        base.OnFrameworkInitializationCompleted();
    }
}

/// <summary>
/// Roboto from the files the other apps load, as <see cref="Launch.ReadFont"/> reads them.
/// </summary>
sealed class DeviceRoboto : FontCollectionBase
{
    public const string Family = "fonts:Device#Roboto";
    static readonly Uri Source = new("fonts:Device");

    public DeviceRoboto()
    {
        foreach (var file in new[] { "Roboto-Regular.ttf", "Roboto-Bold.ttf" })
        {
            // The typeface reads its stream for as long as it lives.
            TryAddGlyphTypeface(new MemoryStream(Launch.ReadFont(file)), out _);
        }
    }

    public override Uri Key => Source;
}
