using Android.App;
using Android.Content.PM;
using Android.OS;
using Android.Runtime;
using Avalonia.Android;

namespace PerfAvalonia;

/// <summary>The gauntlet in Avalonia: `--ei tier N [--ei freeze K]`.</summary>
[Activity(Name = "dev.perfcompare.avalonia.MainActivity", Theme = "@style/PerfTheme", MainLauncher = true, Exported = true, LaunchMode = LaunchMode.SingleTask,
    ConfigurationChanges = ConfigChanges.Orientation | ConfigChanges.KeyboardHidden | ConfigChanges.Keyboard |
        ConfigChanges.ScreenSize | ConfigChanges.SmallestScreenSize | ConfigChanges.ScreenLayout |
        ConfigChanges.UiMode | ConfigChanges.LayoutDirection | ConfigChanges.FontScale | ConfigChanges.Density)]
public class MainActivity : AvaloniaMainActivity
{
    protected override void OnCreate(Bundle? savedInstanceState)
    {
        Launch.Tier = Intent?.GetIntExtra("tier", 5) ?? 5;
        Launch.Freeze = Intent?.GetIntExtra("freeze", 0) ?? 0;
        Launch.Log = line => Android.Util.Log.Info("PerfCompare", line);
        base.OnCreate(savedInstanceState);
    }
}

[Application(AllowBackup = false)]
public class MainApplication(IntPtr handle, JniHandleOwnership ownership)
    : AvaloniaAndroidApplication<App>(handle, ownership);
