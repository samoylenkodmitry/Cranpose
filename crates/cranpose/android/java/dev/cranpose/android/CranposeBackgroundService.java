package dev.cranpose.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
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
     * moments after it opened lands the stop inside the window. The activity
     * arms the record before every start, {@link #enterForeground} clears it,
     * and a stop that arrives while it is armed waits for the service to
     * honour it itself. */
    private static boolean obligationArmed;
    private static boolean stopRequested;

    /** Called by {@link CranposeActivity} immediately before every
     * {@code startForegroundService}, so a stale stop from the previous cycle
     * cannot end the service the moment it comes up. */
    static void noteStartRequested() {
        obligationArmed = true;
        stopRequested = false;
    }

    /** The one way the activity stops this service. */
    static void stop(Context context) {
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
