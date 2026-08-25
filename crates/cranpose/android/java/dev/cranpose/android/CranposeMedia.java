package dev.cranpose.android;

import android.app.Activity;
import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;

/**
 * The media session behind Cranpose's media service.
 *
 * <p>This does not decode anything and does not make a sound. Cranpose decodes
 * in process — see {@code cranpose-media} — because Android's {@code
 * MediaPlayer} plays a file, and a document provider whose bytes come off a
 * network hands back a pipe rather than one. What is here is the half of the
 * stack that only Android has: the {@link AudioManager} focus broker, and the
 * {@link MediaSession} behind the lock screen, the notification and the headset
 * buttons.
 *
 * <p>Everything this learns is pushed to native code through {@link
 * CranposeActivity} — focus when the device changes its mind, a button when one
 * is pressed — and nothing here is polled from the Rust side.
 *
 * <p>Playback outlives the surface: the framework takes a background-work lease
 * while it plays, and {@link CranposeMediaService} keeps the process in the
 * foreground with the notification Android requires for media that carries on
 * with the app off screen.
 */
final class CranposeMedia {
    /** Session states; these mirror the constants in `android_media.rs`. */
    static final int SESSION_STOPPED = 0;
    static final int SESSION_PLAYING = 1;
    static final int SESSION_PAUSED = 2;

    static final int FOCUS_GAINED = 0;
    static final int FOCUS_DUCKED = 1;
    static final int FOCUS_LOST_TRANSIENT = 2;
    static final int FOCUS_LOST = 3;

    static final int COMMAND_PLAY = 0;
    static final int COMMAND_PAUSE = 1;
    static final int COMMAND_TOGGLE = 2;
    static final int COMMAND_STOP = 3;
    static final int COMMAND_NEXT = 4;
    static final int COMMAND_PREVIOUS = 5;
    static final int COMMAND_SEEK = 6;

    /** What native code uses for "this item has no length". */
    private static final long NO_DURATION = -1;

    private final Activity activity;
    private final Handler handler = new Handler(Looper.getMainLooper());

    private MediaSession session;
    private AudioFocusRequest focusRequest;
    private String title = "";
    private String artist = "";
    private long durationMs = NO_DURATION;
    private boolean notificationShown = false;
    private boolean playing = false;

    CranposeMedia(Activity activity) {
        this.activity = activity;
    }

    private AudioManager audioManager() {
        return (AudioManager) activity.getSystemService(Context.AUDIO_SERVICE);
    }

    private static AudioAttributes musicAttributes() {
        return new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                .build();
    }

    // --- Session ------------------------------------------------------------

    /**
     * Publishes where playback is.
     *
     * <p>Android extrapolates the position between updates from the state, the
     * position and the speed it was last given, so this is called when the
     * transport moves rather than on a timer.
     */
    void sessionUpdate(int state, long positionMs, long itemDurationMs, float speed) {
        durationMs = itemDurationMs > 0 ? itemDurationMs : NO_DURATION;
        if (state == SESSION_STOPPED) {
            playing = false;
            releaseSession();
            notificationShown = false;
            CranposeMediaService.stop(activity);
            return;
        }
        playing = state == SESSION_PLAYING;
        openSession();
        applyMetadata();
        session.setPlaybackState(new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY
                        | PlaybackState.ACTION_PAUSE
                        | PlaybackState.ACTION_PLAY_PAUSE
                        | PlaybackState.ACTION_STOP
                        | PlaybackState.ACTION_SEEK_TO
                        | PlaybackState.ACTION_SKIP_TO_NEXT
                        | PlaybackState.ACTION_SKIP_TO_PREVIOUS)
                .setState(
                        playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                        Math.max(0, positionMs),
                        playing ? speed : 0.0f)
                .build());
        // The notification is what keeps the process in the foreground while
        // playback carries on off screen, so it belongs to playing rather than
        // to preparing: an item opened and never started should not post one.
        if (playing || notificationShown) {
            showNotification();
        }
    }

    void setMetadata(String newTitle, String newArtist) {
        title = newTitle == null ? "" : newTitle;
        artist = newArtist == null ? "" : newArtist;
        applyMetadata();
        if (notificationShown) {
            showNotification();
        }
    }

    private void applyMetadata() {
        if (session == null) {
            return;
        }
        session.setMetadata(new android.media.MediaMetadata.Builder()
                .putString(android.media.MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(android.media.MediaMetadata.METADATA_KEY_ARTIST, artist)
                .putLong(android.media.MediaMetadata.METADATA_KEY_DURATION, durationMs)
                .build());
    }

    private void showNotification() {
        notificationShown = true;
        CranposeMediaService.start(activity, title, artist, playing, sessionToken());
    }

    private void openSession() {
        if (session != null) {
            return;
        }
        session = new MediaSession(activity, "cranpose");
        allowMediaButtonsBeforeOreo(session);
        session.setCallback(new MediaSession.Callback() {
            @Override
            public void onPlay() {
                CranposeActivity.onMediaCommand(COMMAND_PLAY, 0);
            }

            @Override
            public void onPause() {
                CranposeActivity.onMediaCommand(COMMAND_PAUSE, 0);
            }

            @Override
            public void onStop() {
                CranposeActivity.onMediaCommand(COMMAND_STOP, 0);
            }

            @Override
            public void onSkipToNext() {
                CranposeActivity.onMediaCommand(COMMAND_NEXT, 0);
            }

            @Override
            public void onSkipToPrevious() {
                CranposeActivity.onMediaCommand(COMMAND_PREVIOUS, 0);
            }

            @Override
            public void onSeekTo(long position) {
                CranposeActivity.onMediaCommand(COMMAND_SEEK, position);
            }
        });
        session.setActive(true);
    }

    /**
     * Android 7 needs to be told a session takes media buttons and transport
     * controls; from Android 8 both are implied and the flags do nothing.
     */
    @SuppressWarnings("deprecation")
    private static void allowMediaButtonsBeforeOreo(MediaSession session) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            return;
        }
        session.setFlags(MediaSession.FLAG_HANDLES_MEDIA_BUTTONS
                | MediaSession.FLAG_HANDLES_TRANSPORT_CONTROLS);
    }

    private MediaSession.Token sessionToken() {
        return session == null ? null : session.getSessionToken();
    }

    private void releaseSession() {
        if (session == null) {
            return;
        }
        session.setActive(false);
        session.release();
        session = null;
    }

    // --- Audio focus --------------------------------------------------------

    private final AudioManager.OnAudioFocusChangeListener focusListener = change -> {
        switch (change) {
            case AudioManager.AUDIOFOCUS_GAIN:
                CranposeActivity.onMediaAudioFocus(FOCUS_GAINED);
                break;
            case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK:
                CranposeActivity.onMediaAudioFocus(FOCUS_DUCKED);
                break;
            case AudioManager.AUDIOFOCUS_LOSS_TRANSIENT:
                CranposeActivity.onMediaAudioFocus(FOCUS_LOST_TRANSIENT);
                break;
            case AudioManager.AUDIOFOCUS_LOSS:
                CranposeActivity.onMediaAudioFocus(FOCUS_LOST);
                break;
            default:
                break;
        }
    };

    /**
     * Asks for audio focus. {@code false} means the broker refused, and the
     * caller must not play: something else owns the output.
     */
    boolean requestFocus() {
        AudioManager manager = audioManager();
        if (manager == null) {
            return true;
        }
        int result;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            focusRequest = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                    .setAudioAttributes(musicAttributes())
                    .setOnAudioFocusChangeListener(focusListener, handler)
                    .setWillPauseWhenDucked(false)
                    .build();
            result = manager.requestAudioFocus(focusRequest);
        } else {
            result = requestFocusBeforeOreo(manager);
        }
        return result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
    }

    /** The focus request Android 7 has; {@link AudioFocusRequest} arrived in 8. */
    @SuppressWarnings("deprecation")
    private int requestFocusBeforeOreo(AudioManager manager) {
        return manager.requestAudioFocus(
                focusListener, AudioManager.STREAM_MUSIC, AudioManager.AUDIOFOCUS_GAIN);
    }

    void abandonFocus() {
        AudioManager manager = audioManager();
        if (manager == null) {
            return;
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            if (focusRequest != null) {
                manager.abandonAudioFocusRequest(focusRequest);
                focusRequest = null;
            }
        } else {
            abandonFocusBeforeOreo(manager);
        }
    }

    /** The counterpart of {@link #requestFocusBeforeOreo}. */
    @SuppressWarnings("deprecation")
    private void abandonFocusBeforeOreo(AudioManager manager) {
        manager.abandonAudioFocus(focusListener);
    }

    // --- Lifecycle ----------------------------------------------------------

    /** Called when the activity goes away: the session must not outlive it. */
    void release() {
        abandonFocus();
        releaseSession();
        if (notificationShown) {
            notificationShown = false;
            CranposeMediaService.stop(activity);
        }
    }
}
