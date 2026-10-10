package dev.cranpose.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/** Foreground execution owned by Cranpose while an app has background work active. */
public final class CranposeBackgroundService extends Service {
    private static final int NOTIFICATION_ID = 0x4352414e;
    private static final String CHANNEL = "cranpose.background";

    /** Main-thread handshake with {@link CranposeActivity}. Stopping a started
     * foreground service before its {@code startForeground} has run is a
     * deliberate framework kill ("Bringing down service while still waiting
     * for start foreground"), and the caller cannot see from outside whether
     * that obligation is still open — a background-work lease that closes
     * moments after it opened lands the stop inside the window. Every
     * accepted {@link #start} arms the record, {@link #enterForeground} clears
     * it, and a stop that arrives while it is armed waits for the service to
     * honour it itself. */
    private static boolean obligationArmed;
    private static boolean stopRequested;
    /** An accepted start that no stop has answered yet. The activity stops on
     * every resume, and an application without the background service does
     * not declare this component, so a stop without this record makes no
     * binder call. */
    private static boolean startOutstanding;

    /** The one way the activity starts this service, on the main thread. The
     * record changes only for a start the platform accepted, and the
     * service's {@code onCreate} runs later on this same thread, so writing
     * the record after the call cannot miss it. A refused start leaves the
     * record of the previous cycle, deferred stop included, as it was. */
    static void start(Context context) {
        Intent service = new Intent(context, CranposeBackgroundService.class);
        ComponentName started;
        try {
            started = Build.VERSION.SDK_INT >= 26
                    ? context.startForegroundService(service)
                    : context.startService(service);
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "background work service could not start", error);
            return;
        }
        if (started == null) {
            android.util.Log.w(
                    "cranpose",
                    "background work needs the \"background\" service in the application's"
                            + " cranpose services");
            return;
        }
        obligationArmed = true;
        stopRequested = false;
        startOutstanding = true;
    }

    /** The one way the activity stops this service. */
    static void stop(Context context) {
        if (!startOutstanding) {
            return;
        }
        startOutstanding = false;
        if (obligationArmed) {
            stopRequested = true;
            return;
        }
        context.stopService(new Intent(context, CranposeBackgroundService.class));
    }

    @Override
    public void onCreate() {
        super.onCreate();
        enterForeground();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        enterForeground();
        return START_NOT_STICKY;
    }

    private void enterForeground() {
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (manager != null && manager.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel channel = new NotificationChannel(
                    CHANNEL, "Background work", NotificationManager.IMPORTANCE_LOW);
            channel.setDescription("Shows when the app is finishing work in the background.");
            manager.createNotificationChannel(channel);
        }
        Notification notification = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_sys_download)
                .setContentTitle(applicationLabel())
                .setContentText("Finishing work…")
                .setOngoing(true)
                .setProgress(0, 0, true)
                .build();
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(
                        NOTIFICATION_ID,
                        notification,
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC);
            } else {
                startForeground(NOTIFICATION_ID, notification);
            }
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "foreground background-work service failed", error);
            stopSelf();
            return;
        }
        obligationArmed = false;
        if (stopRequested) {
            stopRequested = false;
            stopSelf();
        }
    }

    private CharSequence applicationLabel() {
        ApplicationInfo info = getApplicationInfo();
        return getPackageManager().getApplicationLabel(info);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onDestroy() {
        if (Build.VERSION.SDK_INT >= 24) {
            stopForeground(STOP_FOREGROUND_REMOVE);
        } else {
            stopForeground(true);
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
