package dev.cranpose.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
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
 * <p>An application carries this class only when its Gradle build declares
 * {@code cranpose { services.add("microphone-standby") }}.
 */
public final class CranposeMicrophoneStandby extends Service {
    private static final int NOTIFICATION_ID = 0x4d494331;
    private static final String CHANNEL = "cranpose.microphone_standby";

    static void start(Context context) {
        context.startForegroundService(new Intent(context, CranposeMicrophoneStandby.class));
    }

    static void stop(Context context) {
        context.stopService(new Intent(context, CranposeMicrophoneStandby.class));
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (manager != null && manager.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel channel = new NotificationChannel(
                    CHANNEL, "Ready to listen", NotificationManager.IMPORTANCE_LOW);
            channel.setDescription("Shows while the app is ready to listen from the background.");
            manager.createNotificationChannel(channel);
        }
        Notification notification = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_btn_speak_now)
                .setContentTitle(getPackageManager().getApplicationLabel(getApplicationInfo()))
                .setContentText("Ready to listen")
                .setOngoing(true)
                .build();
        try {
            if (Build.VERSION.SDK_INT >= 30) {
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
            } else {
                startForeground(NOTIFICATION_ID, notification);
            }
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "the microphone standby did not start", error);
            stopSelf();
        }
        return START_NOT_STICKY;
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
