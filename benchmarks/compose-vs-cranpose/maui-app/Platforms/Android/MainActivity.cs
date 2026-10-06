using Android.App;
using Android.Content.PM;
using Android.OS;
using Android.Views;

namespace PerfMaui;

/// <summary>The gauntlet in .NET MAUI: `--es scenario gauntlet --ei tier N [--ei freeze K]`.</summary>
[Activity(Name = "dev.perfcompare.maui.MainActivity", Theme = "@style/PerfTheme", MainLauncher = true, Exported = true, LaunchMode = LaunchMode.SingleTask,
    ConfigurationChanges = ConfigChanges.Orientation | ConfigChanges.KeyboardHidden | ConfigChanges.Keyboard |
        ConfigChanges.ScreenSize | ConfigChanges.SmallestScreenSize | ConfigChanges.ScreenLayout |
        ConfigChanges.UiMode | ConfigChanges.LayoutDirection | ConfigChanges.FontScale | ConfigChanges.Density)]
public class MainActivity : MauiAppCompatActivity
{
    protected override void OnCreate(Bundle? savedInstanceState)
    {
        Launch.Tier = Intent?.GetIntExtra("tier", 5) ?? 5;
        Launch.Freeze = Intent?.GetIntExtra("freeze", 0) ?? 0;
        base.OnCreate(savedInstanceState);
        Window?.DecorView.ViewTreeObserver?.AddOnDrawListener(new FirstDraw());
    }

    sealed class FirstDraw : Java.Lang.Object, ViewTreeObserver.IOnDrawListener
    {
        bool logged;

        public void OnDraw()
        {
            if (logged) return;
            logged = true;
            Android.Util.Log.Info(GauntletPage.Tag, "PERF first_frame");
        }
    }
}
