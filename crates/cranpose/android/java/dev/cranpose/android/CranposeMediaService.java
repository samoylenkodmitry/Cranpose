package dev.cranpose.android;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.ServiceInfo;
import android.media.session.MediaSession;
import android.os.Build;
import android.os.IBinder;

/**
 * Foreground execution and the notification Android requires while Cranpose is
 * playing media.
 *
 * <p>Separate from {@link CranposeBackgroundService}, which is typed
 * {@code dataSync} and is what an app that is finishing work uses. Media that
 * carries on with the app off screen has to be typed {@code mediaPlayback} and
 * has to carry transport controls, so it gets its own service rather than a
 * flag on that one.
 *
 * <p>The buttons on the notification come back through
 * {@link CranposeActivity#onMediaCommand}, the same path the lock screen and a
 * headset use, so there is one route into the transport rather than three.
 */
public final class CranposeMediaService extends Service {
    private static final int NOTIFICATION_ID = 0x4352414d;
    private static final String CHANNEL = "cranpose.media";
    private static final String EXTRA_TITLE = "cranpose.media.title";
    private static final String EXTRA_ARTIST = "cranpose.media.artist";
    private static final String EXTRA_PLAYING = "cranpose.media.playing";
    private static final String EXTRA_TOKEN = "cranpose.media.token";
    private static final String ACTION_COMMAND = "cranpose.media.command";
    private static final String EXTRA_COMMAND = "cranpose.media.command.id";

    /** Starts or updates the notification for what is playing now. */
    static void start(
            Context context, String title, String artist, boolean playing, MediaSession.Token token) {
        Intent intent = new Intent(context, CranposeMediaService.class)
                .putExtra(EXTRA_TITLE, title)
                .putExtra(EXTRA_ARTIST, artist)
                .putExtra(EXTRA_PLAYING, playing)
                .putExtra(EXTRA_TOKEN, token);
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent);
            } else {
                context.startService(intent);
            }
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "media foreground service refused", error);
        }
    }

    /** Takes the notification down and lets the process be background again. */
    static void stop(Context context) {
        try {
            context.stopService(new Intent(context, CranposeMediaService.class));
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "media foreground service stop failed", error);
        }
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_COMMAND.equals(intent.getAction())) {
            CranposeActivity.onMediaCommand(intent.getIntExtra(EXTRA_COMMAND, -1), 0);
            return START_NOT_STICKY;
        }
        String title = intent == null ? "" : String.valueOf(intent.getStringExtra(EXTRA_TITLE));
        String artist = intent == null ? "" : String.valueOf(intent.getStringExtra(EXTRA_ARTIST));
        boolean playing = intent != null && intent.getBooleanExtra(EXTRA_PLAYING, false);
        enterForeground(title, artist, playing, sessionToken(intent));
        return START_NOT_STICKY;
    }

    private static MediaSession.Token sessionToken(Intent intent) {
        if (intent == null) {
            return null;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            return intent.getParcelableExtra(EXTRA_TOKEN, MediaSession.Token.class);
        }
        return typedExtraBeforeTiramisu(intent);
    }

    /** The typed {@code getParcelableExtra} arrived in Android 13. */
    @SuppressWarnings("deprecation")
    private static MediaSession.Token typedExtraBeforeTiramisu(Intent intent) {
        return intent.getParcelableExtra(EXTRA_TOKEN);
    }

    private void enterForeground(
            String title, String artist, boolean playing, MediaSession.Token token) {
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (manager != null && manager.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel channel = new NotificationChannel(
                    CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW);
            channel.setDescription("Shows what is playing.");
            manager.createNotificationChannel(channel);
        }

        Notification.MediaStyle style = new Notification.MediaStyle()
                .setShowActionsInCompactView(0, 1, 2);
        if (token != null) {
            style.setMediaSession(token);
        }
        Notification notification = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle(title.isEmpty() ? applicationLabel() : title)
                .setContentText(artist)
                .setOngoing(playing)
                .addAction(action(
                        android.R.drawable.ic_media_previous,
                        "Previous",
                        CranposeMedia.COMMAND_PREVIOUS))
                .addAction(playing
                        ? action(android.R.drawable.ic_media_pause, "Pause",
                                CranposeMedia.COMMAND_PAUSE)
                        : action(android.R.drawable.ic_media_play, "Play",
                                CranposeMedia.COMMAND_PLAY))
                .addAction(action(
                        android.R.drawable.ic_media_next, "Next", CranposeMedia.COMMAND_NEXT))
                .setStyle(style)
                .build();
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(
                        NOTIFICATION_ID,
                        notification,
                        ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
            } else {
                startForeground(NOTIFICATION_ID, notification);
            }
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "media foreground service failed", error);
            stopSelf();
        }
    }

    private Notification.Action action(int icon, String label, int command) {
        Intent intent = new Intent(this, CranposeMediaService.class)
                .setAction(ACTION_COMMAND)
                .putExtra(EXTRA_COMMAND, command);
        PendingIntent pending = PendingIntent.getService(
                this,
                command,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Action.Builder(
                        android.graphics.drawable.Icon.createWithResource(this, icon),
                        label,
                        pending)
                .build();
    }

    private CharSequence applicationLabel() {
        ApplicationInfo info = getApplicationInfo();
        CharSequence label = getPackageManager().getApplicationLabel(info);
        return label == null ? "Playing" : label;
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
