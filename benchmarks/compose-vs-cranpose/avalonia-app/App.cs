using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Media;
using Avalonia.Media.Fonts;
using Avalonia.Styling;
using Avalonia.Themes.Fluent;

namespace PerfAvalonia;

/// <summary>What `am start` asked of the gauntlet.</summary>
public static class Launch
{
    /// <summary>Load tier, 1 to 12.</summary>
    public static int Tier { get; set; } = 5;

    /// <summary>Stop on this frame and hold still, for picture comparisons; 0 runs on.</summary>
    public static int Freeze { get; set; }

    /// <summary>Writes a `PERF` line where `measure.py` reads it.</summary>
    public static Action<string> Log { get; set; } = _ => { };
}

public sealed class App : Avalonia.Application
{
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
        base.OnFrameworkInitializationCompleted();
    }
}

/// <summary>The device's Roboto, the files the other apps load.</summary>
sealed class DeviceRoboto : FontCollectionBase
{
    public const string Family = "fonts:Device#Roboto";
    static readonly Uri Source = new("fonts:Device");

    public DeviceRoboto()
    {
        foreach (var file in new[] { "Roboto-Regular.ttf", "Roboto-Bold.ttf" })
        {
            // The typeface reads its stream for as long as it lives.
            TryAddGlyphTypeface(new MemoryStream(File.ReadAllBytes($"/system/fonts/{file}")), out _);
        }
    }

    public override Uri Key => Source;
}
