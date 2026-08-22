package dev.cranpose.android;

import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageManager;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.MediaMetadataRetriever;
import android.media.MediaPlayer;
import android.media.PlaybackParams;
import android.media.audiofx.Equalizer;
import android.media.audiofx.Visualizer;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;

/**
 * The media stack behind Cranpose's media service.
 *
 * <p>Android already has a decoder, an output route, an audio-focus broker and
 * a lock screen; this wires the framework's one media contract onto them. What
 * happens is pushed to native code through {@link CranposeActivity} — the
 * position, the state, the buttons pressed on the lock screen, the samples a
 * visualiser draws — and nothing here is polled from the Rust side.
 *
 * <p>Playback outlives the surface: the framework takes a background-work lease
 * while it plays, and {@link CranposeMediaService} keeps the process in the
 * foreground with the notification Android requires for media that carries on
 * with the app off screen.
 */
final class CranposeMedia {
    /** Published state kinds; these mirror the constants in `android_media.rs`. */
    static final int STATE_LOADING = 0;
    static final int STATE_READY = 1;
    static final int STATE_PLAYING = 2;
    static final int STATE_PAUSED = 3;
    static final int STATE_ENDED = 4;
    static final int STATE_FAILED = 5;

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

    /** How often the position is pushed while something plays. */
    private static final long PROGRESS_INTERVAL_MS = 250;
    /** How many bytes of waveform the visualiser captures per block. */
    private static final int VISUALIZER_CAPTURE_BYTES = 1024;

    private final Activity activity;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Runnable progressTick = this::pushProgress;

    private MediaPlayer player;
    private MediaSession session;
    private AudioFocusRequest focusRequest;
    private Visualizer visualizer;
    private Equalizer equalizer;
    /** The curve last applied, kept because an effect belongs to one session
     *  and a new item is a new session that has to be given it again. */
    private boolean equalizerEnabled;
    private short equalizerPreampMillibels;
    private short[] equalizerGainsMillibels = new short[0];
    /** What the device's equalizer offers. Probing means creating and releasing
     *  an {@link android.media.audiofx.AudioEffect}, which is far too much work
     *  for a capability question asked on every state change, so the answer is
     *  worked out once. */
    private int[] equalizerCenters;
    private int equalizerRange;
    private String title = "";
    private String artist = "";
    private boolean prepared = false;
    private boolean looping = false;
    private boolean analysisWanted = false;
    /**
     * Whether the playback notification is up.
     *
     * <p>Preparing an item must not post one: nothing is playing yet, and a
     * notification that says otherwise is a notification the user dismisses.
     */
    private boolean notificationShown = false;
    private float volume = 1.0f;
    private float speed = 1.0f;
    private int bufferedPercent = 0;

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

    /**
     * Whether this device will give up samples for a visualiser.
     *
     * <p>{@link Visualizer} reads the output mix, which Android treats as
     * recording, so it needs {@code RECORD_AUDIO}. An application that has not
     * asked for it gets a media player without analysis rather than a silent
     * visualiser, and the framework reports the difference.
     */
    boolean supportsAnalysis() {
        return activity.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO)
                == PackageManager.PERMISSION_GRANTED;
    }

    /**
     * Reads how long the item at {@code uri} is, without opening it for
     * playback.
     *
     * <p>A playlist shows the length of entries nobody has played yet, and
     * {@link MediaMetadataRetriever} is what answers that on Android. Returns
     * {@code -1} where the container carries no duration or cannot be read,
     * which the framework reports as no duration rather than as an error.
     *
     * <p>This reads the file, so native callers run it off the UI thread.
     */
    int probeDurationMs(String uri) {
        MediaMetadataRetriever retriever = new MediaMetadataRetriever();
        try {
            retriever.setDataSource(activity, Uri.parse(uri));
            String value =
                    retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION);
            if (value == null) {
                return -1;
            }
            long duration = Long.parseLong(value.trim());
            return duration > 0 && duration <= Integer.MAX_VALUE ? (int) duration : -1;
        } catch (Exception error) {
            return -1;
        } finally {
            try {
                retriever.release();
            } catch (Exception ignored) {
                // Releasing a retriever that never opened anything is not a
                // failure worth reporting.
            }
        }
    }

    // --- Transport ----------------------------------------------------------

    void prepare(String uri) {
        release();
        CranposeActivity.onMediaState(STATE_LOADING, "");
        try {
            player = new MediaPlayer();
            player.setAudioAttributes(musicAttributes());
            player.setDataSource(activity, Uri.parse(uri));
            player.setLooping(looping);
            player.setVolume(volume, volume);
            player.setOnPreparedListener(prepared -> onPrepared());
            player.setOnCompletionListener(completed -> onCompleted());
            player.setOnBufferingUpdateListener((source, percent) -> bufferedPercent = percent);
            player.setOnErrorListener((source, what, extra) -> {
                CranposeActivity.onMediaState(STATE_FAILED, "media error " + what + "/" + extra);
                return true;
            });
            player.prepareAsync();
        } catch (Exception error) {
            android.util.Log.w("cranpose", "media prepare failed", error);
            CranposeActivity.onMediaState(STATE_FAILED, String.valueOf(error.getMessage()));
            release();
        }
    }

    private void onPrepared() {
        prepared = true;
        applySpeedIfSet();
        openSession();
        // The item's length is only known now, and the lock screen wants it.
        setMetadata(title, artist);
        attachVisualizer();
        // The effect belongs to the session this item just opened, so the
        // curve has to be given to it again rather than surviving from the last.
        attachEqualizer();
        applyEqualizer();
        pushProgress();
        CranposeActivity.onMediaState(STATE_READY, "");
    }

    private void onCompleted() {
        // A looping player restarts by itself and never reports completion, so
        // reaching here means the item is over.
        stopProgress();
        pushProgress();
        showNotification(false);
        updateSessionState(PlaybackState.STATE_STOPPED);
        CranposeActivity.onMediaState(STATE_ENDED, "");
    }

    boolean play() {
        if (player == null || !prepared) {
            return false;
        }
        if (!requestFocus()) {
            return false;
        }
        player.start();
        applySpeedIfSet();
        showNotification(true);
        updateSessionState(PlaybackState.STATE_PLAYING);
        startProgress();
        CranposeActivity.onMediaState(STATE_PLAYING, "");
        return true;
    }

    void pause() {
        if (player != null && prepared && player.isPlaying()) {
            player.pause();
        }
        stopProgress();
        showNotification(false);
        updateSessionState(PlaybackState.STATE_PAUSED);
        CranposeActivity.onMediaState(STATE_PAUSED, "");
    }

    private void showNotification(boolean playing) {
        notificationShown = true;
        CranposeMediaService.start(activity, title, artist, playing, sessionToken());
    }

    void stop() {
        release();
        releaseSession();
        notificationShown = false;
        CranposeMediaService.stop(activity);
    }

    void seekTo(int positionMs) {
        if (player == null || !prepared) {
            return;
        }
        player.seekTo(positionMs);
        pushProgress();
    }

    void setVolume(float value) {
        volume = Math.max(0.0f, Math.min(1.0f, value));
        if (player != null) {
            player.setVolume(volume, volume);
        }
    }

    boolean setSpeed(float value) {
        speed = Math.max(0.25f, Math.min(4.0f, value));
        return applySpeedIfSet();
    }

    private boolean applySpeedIfSet() {
        if (player == null || !prepared) {
            return true;
        }
        try {
            PlaybackParams params = player.getPlaybackParams().setSpeed(speed);
            // Setting the parameters starts a paused player, which is not what
            // a speed control means.
            boolean playing = player.isPlaying();
            player.setPlaybackParams(params);
            if (!playing) {
                player.pause();
            }
            return true;
        } catch (Exception error) {
            android.util.Log.w("cranpose", "media speed refused", error);
            return false;
        }
    }

    void setLooping(boolean value) {
        looping = value;
        if (player != null) {
            player.setLooping(value);
        }
    }

    void setMetadata(String newTitle, String newArtist) {
        title = newTitle == null ? "" : newTitle;
        artist = newArtist == null ? "" : newArtist;
        if (session != null) {
            session.setMetadata(new android.media.MediaMetadata.Builder()
                    .putString(android.media.MediaMetadata.METADATA_KEY_TITLE, title)
                    .putString(android.media.MediaMetadata.METADATA_KEY_ARTIST, artist)
                    .putLong(android.media.MediaMetadata.METADATA_KEY_DURATION, durationMs())
                    .build());
        }
        if (notificationShown) {
            CranposeMediaService.start(activity, title, artist, isPlaying(), sessionToken());
        }
    }

    boolean isPlaying() {
        return player != null && prepared && player.isPlaying();
    }

    private long durationMs() {
        if (player == null || !prepared) {
            return -1;
        }
        int duration = player.getDuration();
        return duration > 0 ? duration : -1;
    }

    // --- Progress -----------------------------------------------------------

    private void startProgress() {
        handler.removeCallbacks(progressTick);
        handler.postDelayed(progressTick, PROGRESS_INTERVAL_MS);
    }

    private void stopProgress() {
        handler.removeCallbacks(progressTick);
    }

    private void pushProgress() {
        if (player == null || !prepared) {
            return;
        }
        long duration = durationMs();
        long buffered = duration > 0 ? duration * bufferedPercent / 100 : 0;
        CranposeActivity.onMediaProgress(player.getCurrentPosition(), duration, buffered);
        // Whoever asked, there is one chain: a seek during playback would
        // otherwise start a second one and double the rate for good.
        handler.removeCallbacks(progressTick);
        if (player.isPlaying()) {
            handler.postDelayed(progressTick, PROGRESS_INTERVAL_MS);
        }
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

    private boolean requestFocus() {
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

    private void abandonFocus() {
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

    // --- Media session ------------------------------------------------------

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

    private void updateSessionState(int state) {
        if (session == null) {
            return;
        }
        long position = player != null && prepared ? player.getCurrentPosition() : 0;
        session.setPlaybackState(new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY
                        | PlaybackState.ACTION_PAUSE
                        | PlaybackState.ACTION_PLAY_PAUSE
                        | PlaybackState.ACTION_STOP
                        | PlaybackState.ACTION_SEEK_TO
                        | PlaybackState.ACTION_SKIP_TO_NEXT
                        | PlaybackState.ACTION_SKIP_TO_PREVIOUS)
                .setState(state, position, state == PlaybackState.STATE_PLAYING ? speed : 0.0f)
                .build());
    }

    // --- Analysis -----------------------------------------------------------

    boolean setAnalysisEnabled(boolean enabled) {
        analysisWanted = enabled;
        if (!enabled) {
            releaseVisualizer();
            return true;
        }
        if (!supportsAnalysis()) {
            return false;
        }
        attachVisualizer();
        return visualizer != null;
    }

    private void attachVisualizer() {
        if (!analysisWanted || visualizer != null || player == null || !prepared) {
            return;
        }
        if (!supportsAnalysis()) {
            return;
        }
        try {
            Visualizer created = new Visualizer(player.getAudioSessionId());
            int capture = Math.min(VISUALIZER_CAPTURE_BYTES, Visualizer.getCaptureSizeRange()[1]);
            created.setCaptureSize(capture);
            created.setDataCaptureListener(new Visualizer.OnDataCaptureListener() {
                @Override
                public void onWaveFormDataCapture(Visualizer source, byte[] waveform, int rate) {
                    CranposeActivity.onMediaSamples(waveform, rate / 1000);
                }

                @Override
                public void onFftDataCapture(Visualizer source, byte[] fft, int rate) {
                    // The framework publishes the waveform; a spectrum is the
                    // application's own transform of it.
                }
            }, Visualizer.getMaxCaptureRate(), true, false);
            created.setEnabled(true);
            visualizer = created;
        } catch (Exception error) {
            android.util.Log.w("cranpose", "visualizer unavailable", error);
            releaseVisualizer();
        }
    }

    // --- Equalizer ----------------------------------------------------------

    /**
     * The band centre frequencies this device's equalizer has, in hertz.
     *
     * <p>An {@link Equalizer} has the bands its implementation has -- five on
     * most devices -- rather than the ten a graphic equalizer is drawn with, so
     * the framework reports the real ones and lets the application map onto
     * them. An empty array means there is no equalizer here at all.
     */
    int[] equalizerBandCenters() {
        probeEqualizer();
        return equalizerCenters;
    }

    /** The most a band can lift or cut on this device, in millibels. */
    int equalizerRangeMillibels() {
        probeEqualizer();
        return equalizerRange;
    }

    private void probeEqualizer() {
        if (equalizerCenters != null) {
            return;
        }
        Equalizer probe = null;
        try {
            // Session 0 is the output mix. Attaching to it is enough to ask
            // what the implementation offers without an item being open.
            probe = new Equalizer(0, 0);
            short count = probe.getNumberOfBands();
            int[] centers = new int[count];
            for (short band = 0; band < count; band++) {
                // The platform reports millihertz.
                centers[band] = probe.getCenterFreq(band) / 1000;
            }
            short[] range = probe.getBandLevelRange();
            equalizerRange = Math.min(Math.abs(range[0]), Math.abs(range[1]));
            equalizerCenters = centers;
        } catch (Exception error) {
            android.util.Log.w("cranpose", "equalizer unavailable", error);
            equalizerCenters = new int[0];
            equalizerRange = 0;
        } finally {
            if (probe != null) {
                try {
                    probe.release();
                } catch (Exception ignored) {
                    // Releasing a probe that never attached is not a failure.
                }
            }
        }
    }

    /**
     * Applies a curve. Remembered and reapplied to each item, because the
     * effect is attached to the audio session an item opens.
     */
    void setEqualizer(boolean enabled, int preampMillibels, int[] gainsMillibels) {
        equalizerEnabled = enabled;
        equalizerPreampMillibels = (short) preampMillibels;
        equalizerGainsMillibels = new short[gainsMillibels.length];
        for (int index = 0; index < gainsMillibels.length; index++) {
            equalizerGainsMillibels[index] = (short) gainsMillibels[index];
        }
        attachEqualizer();
        applyEqualizer();
    }

    private void attachEqualizer() {
        if (equalizer != null || player == null || !prepared) {
            return;
        }
        try {
            equalizer = new Equalizer(0, player.getAudioSessionId());
        } catch (Exception error) {
            android.util.Log.w("cranpose", "equalizer unavailable", error);
            equalizer = null;
        }
    }

    private void applyEqualizer() {
        if (equalizer == null) {
            return;
        }
        try {
            equalizer.setEnabled(equalizerEnabled);
            if (!equalizerEnabled) {
                return;
            }
            short count = equalizer.getNumberOfBands();
            short[] range = equalizer.getBandLevelRange();
            for (short band = 0; band < count; band++) {
                short gain = band < equalizerGainsMillibels.length
                        ? equalizerGainsMillibels[band]
                        : 0;
                // The platform equalizer has no preamp of its own, so the
                // preamp rides on every band -- which is what a preamp is.
                equalizer.setBandLevel(band, clampBandLevel(
                        range, (short) (gain + equalizerPreampMillibels)));
            }
        } catch (Exception error) {
            android.util.Log.w("cranpose", "equalizer apply failed", error);
        }
    }

    private short clampBandLevel(short[] range, short level) {
        if (level < range[0]) {
            return range[0];
        }
        if (level > range[1]) {
            return range[1];
        }
        return level;
    }

    private void releaseEqualizer() {
        if (equalizer == null) {
            return;
        }
        try {
            equalizer.setEnabled(false);
            equalizer.release();
        } catch (Exception error) {
            android.util.Log.w("cranpose", "equalizer release failed", error);
        }
        equalizer = null;
    }

    private void releaseVisualizer() {
        if (visualizer == null) {
            return;
        }
        try {
            visualizer.setEnabled(false);
            visualizer.release();
        } catch (Exception error) {
            android.util.Log.w("cranpose", "visualizer release failed", error);
        }
        visualizer = null;
    }

    // --- Teardown -----------------------------------------------------------

    private void releaseSession() {
        if (session == null) {
            return;
        }
        session.setActive(false);
        session.release();
        session = null;
    }

    /**
     * Releases the item and the device, keeping the media session: a playlist
     * advancing replaces what is playing, not what the lock screen is.
     */
    private void release() {
        stopProgress();
        releaseVisualizer();
        releaseEqualizer();
        abandonFocus();
        if (player != null) {
            try {
                player.reset();
            } catch (Exception error) {
                android.util.Log.w("cranpose", "media reset failed", error);
            }
            player.release();
            player = null;
        }
        prepared = false;
        bufferedPercent = 0;
    }
}
