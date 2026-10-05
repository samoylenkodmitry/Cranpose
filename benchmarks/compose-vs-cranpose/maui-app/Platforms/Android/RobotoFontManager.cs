using Android.Graphics;

namespace PerfMaui;

/// <summary>
/// Every label's typeface: the device's Roboto files, the ones the other apps
/// load, regular or bold. Sizes stay MAUI's own.
/// </summary>
sealed class RobotoFontManager(IFontRegistrar registrar, IServiceProvider services) : IFontManager
{
    readonly FontManager platform = new(registrar, services);
    readonly Typeface regular = Load("Roboto-Regular.ttf", TypefaceStyle.Normal);
    readonly Typeface bold = Load("Roboto-Bold.ttf", TypefaceStyle.Bold);

    static Typeface Load(string file, TypefaceStyle fallback) =>
        Typeface.CreateFromFile($"/system/fonts/{file}") ?? Typeface.DefaultFromStyle(fallback);

    public double DefaultFontSize => platform.DefaultFontSize;

    public Typeface DefaultTypeface => regular;

    public Typeface? GetTypeface(Microsoft.Maui.Font font) => font.Weight >= FontWeight.Bold ? bold : regular;

    public FontSize GetFontSize(Microsoft.Maui.Font font, float defaultFontSize = 0) => platform.GetFontSize(font, defaultFontSize);
}
