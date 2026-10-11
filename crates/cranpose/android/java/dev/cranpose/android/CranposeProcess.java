package dev.cranpose.android;

import android.content.ComponentName;
import android.content.Context;

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
 *
 * <p>Crates such as cpal read the application context from {@code ndk_context},
 * which the activity's native glue sets once per process and refuses to set
 * twice. A start in a process with no activity sets it, and
 * {@link #beforeActivity} hands it back before the activity sets it again.
 * Both run on the main thread.
 */
public final class CranposeProcess {
    private static boolean started;
    private static boolean activityCreated;
    private static boolean sharedContext;

    private CranposeProcess() {}

    private static native void nativeStart(Context context, boolean share);

    private static native void nativeYieldContext();

    /** Starts the native side once; whether the native library is loaded. */
    public static synchronized boolean start(Context context) {
        if (!started
                && CranposeActivity.loadNativeLibrary(
                        context, new ComponentName(context, CranposeActivity.class))) {
            sharedContext = !activityCreated;
            nativeStart(context.getApplicationContext(), sharedContext);
            started = true;
        }
        return started;
    }

    /** Called by an activity before its native glue starts. */
    static synchronized void beforeActivity() {
        activityCreated = true;
        if (sharedContext) {
            sharedContext = false;
            nativeYieldContext();
        }
    }
}
