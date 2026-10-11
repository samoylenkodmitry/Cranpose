package dev.cranpose.android;

import android.content.ComponentName;
import android.content.Context;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.util.Log;

/**
 * Starts the application's native side in a process the system started for
 * a service, with no activity.
 *
 * <p>A service calls {@link #start} before it hands Rust anything. It loads
 * the library the activity names and runs, once per process, what the
 * application does with or without a window: the services that need only the
 * application context and the application's
 * {@code AppLauncher::with_process_start}. When the activity starts later in
 * the same process, that start has already run and is not repeated.
 */
public final class CranposeProcess {
    private static final String TAG = "CranposeProcess";
    private static boolean started;

    private CranposeProcess() {}

    private static native void nativeStart(Context context);

    /** Starts the native side once; whether the native library is loaded. */
    public static synchronized boolean start(Context context) {
        if (!started && loadLibrary(context)) {
            nativeStart(context.getApplicationContext());
            started = true;
        }
        return started;
    }

    private static boolean loadLibrary(Context context) {
        try {
            ActivityInfo activity = context.getPackageManager().getActivityInfo(
                    new ComponentName(context, CranposeActivity.class), PackageManager.GET_META_DATA);
            String library = activity.metaData == null ? null : activity.metaData.getString("android.app.lib_name");
            if (library == null) {
                return false;
            }
            System.loadLibrary(library);
            return true;
        } catch (PackageManager.NameNotFoundException | UnsatisfiedLinkError error) {
            Log.w(TAG, "the application's native library did not load", error);
            return false;
        }
    }
}
