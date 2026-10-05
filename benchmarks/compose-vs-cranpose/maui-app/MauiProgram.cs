using Microsoft.Extensions.DependencyInjection.Extensions;

namespace PerfMaui;

public static class MauiProgram
{
    public static MauiApp CreateMauiApp()
    {
        var builder = MauiApp.CreateBuilder().UseMauiApp<App>();
#if ANDROID
        builder.Services.Replace(ServiceDescriptor.Singleton<IFontManager, RobotoFontManager>());
        // Text without the font's extra padding above and below, as Compose sets it.
        Microsoft.Maui.Handlers.LabelHandler.Mapper.AppendToMapping(
            "NoFontPadding", (handler, _) => handler.PlatformView.SetIncludeFontPadding(false));
#endif
        return builder.Build();
    }
}

public sealed class App : Application
{
    public App()
    {
        // The window is fullscreen and already starts below the display
        // cutout, as the other apps' windows do: no view insets itself again.
        var edgeToEdge = new SafeAreaEdges(SafeAreaRegions.None);
        foreach (var (type, property) in new (Type, BindableProperty)[]
                 {
                     (typeof(Layout), Layout.SafeAreaEdgesProperty),
                     (typeof(Border), Border.SafeAreaEdgesProperty),
                     (typeof(ContentView), ContentView.SafeAreaEdgesProperty),
                 })
        {
            Resources.Add(new Style(type)
            {
                ApplyToDerivedTypes = true,
                Setters = { new Setter { Property = property, Value = edgeToEdge } },
            });
        }
    }

    protected override Window CreateWindow(IActivationState? activationState) =>
        new(new GauntletPage(Launch.Tier, Launch.Freeze));
}

/// <summary>What `am start` asked of the gauntlet.</summary>
public static class Launch
{
    /// <summary>Load tier, 1 to 8.</summary>
    public static int Tier { get; set; } = 5;

    /// <summary>Stop on this frame and hold still, for picture comparisons; 0 runs on.</summary>
    public static int Freeze { get; set; }
}
