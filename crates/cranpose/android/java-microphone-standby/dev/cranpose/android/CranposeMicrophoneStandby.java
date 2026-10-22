package dev.cranpose.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/**
 * Keeps the microphone usable while the application is in the background,
 * behind {@code cranpose_services::hold_microphone_standby}.
 *
 * <p>Android lets a background application open the microphone only from a
 * foreground service of type microphone that was started while the
 * application was in front. This service is that standby: the application
 * starts it from its screen, and may then record later, for example when a
 * paired watch asks, while the phone is in a pocket. The service itself
 * records nothing; the system's microphone indicator shows only while the
 * application records.
 *
 * <p>The notification says what the application does now and has an action
 * that ends the standby, both in the application's language
 * ({@code cranpose_services::set_microphone_standby_notice}). The action
 * stops the service and tells the application through
 * {@code nativeOnStandbyStop}; a tap on the notification opens the app.
 *
 * <p>An application carries this class only when its Gradle build declares
 * {@code cranpose { services.add("microphone-standby") }}.
 */
public final class CranposeMicrophoneStandby extends Service {
    private static final int NOTIFICATION_ID = 0x4d494331;
    private static final String CHANNEL = "cranpose.microphone_standby";
    private static final String STOP = "dev.cranpose.android.MICROPHONE_STANDBY_STOP";

    private static volatile String text = "Ready to listen";
    private static volatile String stopLabel = null;
    private static volatile boolean running = false;

    private static native void nativeOnStandbyStop();

    static void start(Context context) {
        context.startForegroundService(new Intent(context, CranposeMicrophoneStandby.class));
    }

    static void stop(Context context) {
        context.stopService(new Intent(context, CranposeMicrophoneStandby.class));
    }

    /** What the notification says and how its action is named; a showing
     * notification changes at once. */
    static void setNotice(Context context, String noticeText, String noticeStop) {
        text = noticeText;
        stopLabel = noticeStop;
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (running && manager != null) {
            manager.notify(NOTIFICATION_ID, notification(context));
        }
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && STOP.equals(intent.getAction())) {
            stopSelf();
            nativeOnStandbyStop();
            return START_NOT_STICKY;
        }
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (manager != null && manager.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel channel = new NotificationChannel(
                    CHANNEL, "Ready to listen", NotificationManager.IMPORTANCE_LOW);
            channel.setDescription("Shows while the app is ready to listen from the background.");
            manager.createNotificationChannel(channel);
        }
        Notification notification = notification(this);
        try {
            if (Build.VERSION.SDK_INT >= 30) {
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
            } else {
                startForeground(NOTIFICATION_ID, notification);
            }
            running = true;
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "the microphone standby did not start", error);
            stopSelf();
        }
        return START_NOT_STICKY;
    }

    /** The app's name over what it does now, with the action that ends the
     * standby; a tap opens the app. */
    private static Notification notification(Context context) {
        Notification.Builder builder = new Notification.Builder(context, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_btn_speak_now)
                .setContentTitle(context.getPackageManager().getApplicationLabel(context.getApplicationInfo()))
                .setContentText(text)
                .setOngoing(true)
                .setOnlyAlertOnce(true);
        Intent launch = context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
        if (launch != null) {
            launch.setFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED);
            builder.setContentIntent(PendingIntent.getActivity(
                    context, 0, launch, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT));
        }
        String label = stopLabel;
        if (label != null) {
            Intent stop = new Intent(context, CranposeMicrophoneStandby.class).setAction(STOP);
            PendingIntent pending = PendingIntent.getService(
                    context, 1, stop, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
            builder.addAction(new Notification.Action.Builder(null, label, pending).build());
        }
        return builder.build();
    }

    @Override
    public void onDestroy() {
        running = false;
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
