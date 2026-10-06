using Android.App;
using Android.Content.PM;
using Android.OS;
using Android.Runtime;

namespace PerfUno.Droid;

/// <summary>The gauntlet in Uno: `--ei tier N [--ei freeze K]`.</summary>
[Activity(Name = "dev.perfcompare.uno.MainActivity", Theme = "@style/PerfTheme", MainLauncher = true, Exported = true,
    LaunchMode = LaunchMode.SingleTask, ConfigurationChanges = global::Uno.UI.ActivityHelper.AllConfigChanges)]
public class MainActivity : Microsoft.UI.Xaml.ApplicationActivity
{
    protected override void OnCreate(Bundle? savedInstanceState)
    {
        Launch.Tier = Intent?.GetIntExtra("tier", 5) ?? 5;
        Launch.Freeze = Intent?.GetIntExtra("freeze", 0) ?? 0;
        Launch.Log = line => Android.Util.Log.Info("PerfCompare", line);
        base.OnCreate(savedInstanceState);
    }
}

[Application(AllowBackup = false, HardwareAccelerated = true)]
public class MainApplication(IntPtr handle, JniHandleOwnership ownership)
    : Microsoft.UI.Xaml.NativeApplication(() => new App(), handle, ownership);
