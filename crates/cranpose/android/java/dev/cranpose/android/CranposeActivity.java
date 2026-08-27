package dev.cranpose.android;

import android.app.NativeActivity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.pm.PackageInstaller;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.graphics.Rect;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.os.Build;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.os.VibratorManager;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.WindowManager;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;
import android.view.WindowInsets;
import android.content.UriPermission;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;

import org.json.JSONArray;
import org.json.JSONObject;

/**
 * A {@link NativeActivity} that exposes the Storage Access Framework to
 * Cranpose's native file picker.
 *
 * <p>Apps that want {@code cranpose_services::file_picker} on Android declare
 * this class (or a subclass) as their launcher activity. The Rust backend calls
 * {@link #cranposePickFile(long)} / {@link #cranposePickFolder(long)} /
 * {@link #cranposePickFolderStreaming(long)} over JNI; the chosen
 * {@code content://} document URIs are reported back <em>without copying any
 * data</em>. A picked file is later opened on demand through
 * {@link #cranposeOpenUri(String)}, which hands the provider's descriptor to
 * Rust, so even a multi-gigabyte folder is selected instantly and each file is
 * streamed only when it is actually read.
 *
 * <p>The folder picker uses {@code ACTION_OPEN_DOCUMENT_TREE}, so the user can
 * choose a folder served by any document provider the device exposes — local
 * storage, cloud, or a mounted WebDAV share — rather than a private path.
 *
 * <p>{@link #cranposePickFolderStreaming(long)} resolves the selection at once
 * and then walks the tree on a worker thread, reporting files in batches as it
 * finds them ({@link #nativeOnFolderEntries}). A slow provider (a mounted WebDAV
 * share with thousands of files) therefore no longer freezes the app: the first
 * tracks appear and can be played while the rest keep streaming in.
 */
public class CranposeActivity extends NativeActivity {
    private static native boolean nativeOnBackInvoked();
    private static native void nativeOnIncomingContent(String name, String mimeType, String uri);
    private static native void nativeOnAppUpdateStatus(int kind, String version,
            String downloadUrl, long downloaded, long total, String message, String digest);
    private static native void nativeOnCameraFrame(byte[] nv12, int width, int height,
            int rotationDegrees, long sequence);
    private static native void nativeOnCameraFrameDropped();
    private static native void nativeOnCameraState(int kind, String detail);
    private static native void nativeOnCameraStill(byte[] jpeg, String error);

    /** One preview frame, in the format the sensor produced. */
    static void onCameraFrame(
            byte[] nv12, int width, int height, int rotationDegrees, long sequence) {
        nativeOnCameraFrame(nv12, width, height, rotationDegrees, sequence);
    }

    /** A frame the device produced while the previous one was still in flight. */
    static void onCameraFrameDropped() {
        nativeOnCameraFrameDropped();
    }

    /** The session is producing frames from {@code device}. */
    static void onCameraRunning(String device) {
        nativeOnCameraState(CAMERA_RUNNING, device);
    }

    /** The session could not open, or ended on its own. */
    static void onCameraFailed(String detail) {
        nativeOnCameraState(CAMERA_FAILED, detail == null ? "" : detail);
    }

    /** The session stopped and the device was released. */
    static void onCameraStopped() {
        nativeOnCameraState(CAMERA_STOPPED, "");
    }

    /** A still, or the reason there is none. */
    static void onCameraStill(byte[] jpeg, String error) {
        nativeOnCameraStill(jpeg, error == null ? "" : error);
    }

    private static final int CAMERA_RUNNING = 1;
    private static final int CAMERA_STOPPED = 2;
    private static final int CAMERA_FAILED = 3;

    private static native void nativeOnMediaAudioFocus(int focus);
    private static native void nativeOnMediaCommand(int command, long positionMs);

    /** What the rest of the device is doing with the output. */
    static void onMediaAudioFocus(int focus) {
        nativeOnMediaAudioFocus(focus);
    }

    /** A button pressed on the lock screen, the notification or a headset. */
    static void onMediaCommand(int command, long positionMs) {
        nativeOnMediaCommand(command, positionMs);
    }
    public void cranposeSetKeepScreenOn(boolean enabled) {
        if (enabled) {
            getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        } else {
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        }
    }
    public void cranposeMoveToBackground() { moveTaskToBack(true); }

    public int cranposeThermalStatus() {
        if (Build.VERSION.SDK_INT < 29) {
            return 0;
        }
        android.os.PowerManager manager = getSystemService(android.os.PowerManager.class);
        return manager == null ? 0 : manager.getCurrentThermalStatus();
    }

    public int cranposeBatteryStatus() {
        try {
            Intent battery = registerReceiver(
                    null, new android.content.IntentFilter(Intent.ACTION_BATTERY_CHANGED));
            if (battery == null) {
                return 0x100 | 100;
            }
            int level = battery.getIntExtra(android.os.BatteryManager.EXTRA_LEVEL, -1);
            int scale = battery.getIntExtra(android.os.BatteryManager.EXTRA_SCALE, -1);
            int percent = level >= 0 && scale > 0 ? level * 100 / scale : 100;
            int status = battery.getIntExtra(android.os.BatteryManager.EXTRA_STATUS, -1);
            boolean charging = status == android.os.BatteryManager.BATTERY_STATUS_CHARGING
                    || status == android.os.BatteryManager.BATTERY_STATUS_FULL;
            return (percent & 0xff) | (charging ? 0x100 : 0);
        } catch (Exception error) {
            return 0x100 | 100;
        }
    }

    public boolean cranposeUnrestrictedBackgroundWork() {
        android.os.PowerManager manager = getSystemService(android.os.PowerManager.class);
        return manager == null || manager.isIgnoringBatteryOptimizations(getPackageName());
    }

    public void cranposeRequestUnrestrictedBackgroundWork() {
        runOnUiThread(() -> {
            Intent settings = new Intent(
                    android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS);
            try {
                startActivity(settings);
            } catch (android.content.ActivityNotFoundException first) {
                try {
                    startActivity(new Intent(android.provider.Settings.ACTION_SETTINGS));
                } catch (android.content.ActivityNotFoundException ignored) {
                }
            }
        });
    }

    public byte[] cranposeReadBundledAsset(String path) {
        try (InputStream input = getAssets().open(path);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[65536];
            int read;
            while ((read = input.read(buffer)) != -1) {
                output.write(buffer, 0, read);
            }
            return output.toByteArray();
        } catch (IOException error) {
            return null;
        }
    }

    private static final int UPDATE_CHECKING = 1;
    private static final int UPDATE_CURRENT = 2;
    private static final int UPDATE_AVAILABLE = 3;
    private static final int UPDATE_DOWNLOADING = 4;
    private static final int UPDATE_CONFIRMATION = 5;
    private static final int UPDATE_INSTALLING = 6;
    private static final int UPDATE_ERROR = 7;
    private static final int UPDATE_VERIFYING = 8;

    private String cranposeUpdateInstallAction() {
        return getPackageName() + ".CRANPOSE_UPDATE_INSTALL_RESULT";
    }

    private final BroadcastReceiver cranposeUpdateInstallReceiver = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            int status = intent.getIntExtra(
                    PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
            if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                nativeOnAppUpdateStatus(UPDATE_CONFIRMATION, "", "", 0, 0, "", "");
                Intent confirmation;
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    confirmation = intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class);
                } else {
                    @SuppressWarnings("deprecation")
                    Intent value = intent.getParcelableExtra(Intent.EXTRA_INTENT);
                    confirmation = value;
                }
                if (confirmation != null) {
                    confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    try {
                        startActivity(confirmation);
                    } catch (Exception error) {
                        cranposeUpdateError(error);
                    }
                }
            } else if (status == PackageInstaller.STATUS_SUCCESS) {
                nativeOnAppUpdateStatus(UPDATE_INSTALLING, "", "", 0, 0, "", "");
            } else {
                String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
                nativeOnAppUpdateStatus(UPDATE_ERROR, "", "", 0, 0,
                        message == null ? "installation failed" : message, "");
            }
        }
    };

    /** Queries a GitHub repository's latest release and selects one package asset. */
    public void cranposeCheckGitHubUpdate(
            String repository, String currentVersion, String assetSuffix) {
        final String repo = repository == null ? "" : repository.trim();
        final String current = currentVersion == null ? "" : currentVersion.trim();
        final String suffix = assetSuffix == null ? "" : assetSuffix.trim();
        new Thread(() -> {
            try {
                if (!repo.matches("[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")) {
                    throw new IllegalArgumentException("invalid GitHub repository");
                }
                nativeOnAppUpdateStatus(UPDATE_CHECKING, "", "", 0, 0, "", "");
                URL url = new URL("https://api.github.com/repos/" + repo + "/releases/latest");
                HttpURLConnection connection = (HttpURLConnection) url.openConnection();
                connection.setRequestProperty("User-Agent", getPackageName());
                connection.setRequestProperty("Accept", "application/vnd.github+json");
                connection.setConnectTimeout(15000);
                connection.setReadTimeout(15000);
                int code = connection.getResponseCode();
                if (code != HttpURLConnection.HTTP_OK) {
                    throw new IOException("GitHub returned HTTP " + code);
                }
                JSONObject release = new JSONObject(cranposeReadText(connection.getInputStream()));
                String tag = release.optString("tag_name", "");
                String latest = tag.startsWith("v") ? tag.substring(1) : tag;
                String packageUrl = "";
                long packageSize = 0;
                // GitHub publishes an asset's digest as `sha256:<hex>`, which is
                // the form the framework parses. An asset without one is still
                // offered, and refused at install time if it cannot be checked.
                String packageDigest = "";
                JSONArray assets = release.optJSONArray("assets");
                if (assets != null) {
                    for (int index = 0; index < assets.length(); index++) {
                        JSONObject asset = assets.getJSONObject(index);
                        if (asset.optString("name", "").endsWith(suffix)) {
                            packageUrl = asset.optString("browser_download_url", "");
                            packageSize = asset.optLong("size", 0);
                            packageDigest = asset.optString("digest", "");
                            break;
                        }
                    }
                }
                if (latest.isEmpty() || packageUrl.isEmpty()) {
                    throw new IOException("latest release has no matching package");
                }
                if (cranposeIsNewerVersion(latest, current)) {
                    nativeOnAppUpdateStatus(
                            UPDATE_AVAILABLE, latest, packageUrl, 0, packageSize, "",
                            packageDigest);
                } else {
                    nativeOnAppUpdateStatus(UPDATE_CURRENT, "", "", 0, 0, "", "");
                }
            } catch (Exception error) {
                cranposeUpdateError(error);
            }
        }, "cranpose-update-check").start();
    }

    /**
     * Downloads a package, checks it against the digest the release feed
     * published, and hands it to Android's platform installer.
     *
     * <p>The digest is checked <em>before</em> the session is committed, and the
     * session is abandoned when it does not match, so a package that arrived
     * corrupted or was swapped in transit never reaches the installer. Android's
     * own signature check still applies afterwards and catches a package signed
     * by someone else; it does not catch one that arrived damaged.
     *
     * @param downloadUrl where the package is fetched from
     * @param digestSpec  {@code sha256:<hex>}; the framework refuses a package
     *                    without one before this is called
     * @param expectedSize the size the feed published, or {@code 0}
     */
    public void cranposeInstallUpdate(String downloadUrl, String digestSpec, long expectedSize) {
        final String source = downloadUrl == null ? "" : downloadUrl.trim();
        final String digestRequest = digestSpec == null ? "" : digestSpec.trim();
        final long announcedSize = Math.max(expectedSize, 0);
        new Thread(() -> {
            PackageInstaller.Session session = null;
            try {
                MessageDigest digest = cranposeUpdateDigest(digestRequest);
                HttpURLConnection connection = (HttpURLConnection) new URL(source).openConnection();
                connection.setRequestProperty("User-Agent", getPackageName());
                connection.setInstanceFollowRedirects(true);
                connection.setConnectTimeout(15000);
                connection.setReadTimeout(30000);
                int code = connection.getResponseCode();
                if (code != HttpURLConnection.HTTP_OK) {
                    throw new IOException("package download returned HTTP " + code);
                }
                long declared = connection.getContentLengthLong();
                long total = declared > 0 ? declared : announcedSize;
                nativeOnAppUpdateStatus(UPDATE_DOWNLOADING, "", "", 0, total, "", "");

                PackageInstaller installer = getPackageManager().getPackageInstaller();
                PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(
                        PackageInstaller.SessionParams.MODE_FULL_INSTALL);
                if (total > 0) {
                    params.setSize(total);
                }
                int sessionId = installer.createSession(params);
                session = installer.openSession(sessionId);
                long written = 0;
                try (InputStream input = connection.getInputStream();
                        OutputStream output = session.openWrite("cranpose-update", 0, total)) {
                    byte[] buffer = new byte[65536];
                    int read;
                    while ((read = input.read(buffer)) >= 0) {
                        output.write(buffer, 0, read);
                        digest.update(buffer, 0, read);
                        written += read;
                        nativeOnAppUpdateStatus(
                                UPDATE_DOWNLOADING, "", "", written, total, "", "");
                    }
                    session.fsync(output);
                }

                nativeOnAppUpdateStatus(UPDATE_VERIFYING, "", "", written, total, "", "");
                String actual = cranposeHex(digest.digest());
                String expected = cranposeDigestValue(digestRequest);
                if (!actual.equals(expected)) {
                    throw new IOException(
                            "the downloaded package does not match its digest (expected "
                                    + expected + ", got " + actual + ")");
                }
                if (announcedSize > 0 && written != announcedSize) {
                    throw new IOException("the downloaded package is " + written
                            + " bytes, and the release feed said " + announcedSize);
                }

                Intent result = new Intent(cranposeUpdateInstallAction()).setPackage(getPackageName());
                int flags = PendingIntent.FLAG_UPDATE_CURRENT;
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    flags |= PendingIntent.FLAG_MUTABLE;
                }
                PendingIntent pending = PendingIntent.getBroadcast(this, sessionId, result, flags);
                session.commit(pending.getIntentSender());
                session.close();
                session = null;
                nativeOnAppUpdateStatus(UPDATE_INSTALLING, "", "", 0, total, "", "");
            } catch (Exception error) {
                if (session != null) {
                    session.abandon();
                    session.close();
                }
                cranposeUpdateError(error);
            }
        }, "cranpose-update-install").start();
    }

    /**
     * The digest engine for a {@code sha256:<hex>} request.
     *
     * <p>A missing digest, or one in a form this platform cannot compute, is a
     * failure rather than a skip: a check nobody performs reads as a package
     * that was verified. The framework refuses a package without a digest
     * before the download starts, so reaching here without one is a bug rather
     * than a release feed's omission.
     */
    private static MessageDigest cranposeUpdateDigest(String digestSpec) throws IOException {
        if (digestSpec.isEmpty()) {
            throw new IOException("the package carries no digest, so it cannot be checked");
        }
        int separator = digestSpec.indexOf(':');
        if (separator <= 0) {
            throw new IOException("unreadable package digest: " + digestSpec);
        }
        String algorithm = digestSpec.substring(0, separator).trim().toLowerCase(Locale.ROOT);
        if (!algorithm.equals("sha256") && !algorithm.equals("sha-256")) {
            throw new IOException("unsupported package digest algorithm: " + algorithm);
        }
        if (cranposeDigestValue(digestSpec).isEmpty()) {
            throw new IOException("empty package digest: " + digestSpec);
        }
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException error) {
            throw new IOException("this device cannot compute SHA-256", error);
        }
    }

    /** The hexadecimal half of a {@code sha256:<hex>} request, lower-cased. */
    private static String cranposeDigestValue(String digestSpec) {
        int separator = digestSpec.indexOf(':');
        if (separator < 0) {
            return "";
        }
        return digestSpec.substring(separator + 1).trim().toLowerCase(Locale.ROOT);
    }

    private static String cranposeHex(byte[] bytes) {
        StringBuilder out = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            out.append(Character.forDigit((value >> 4) & 0x0f, 16));
            out.append(Character.forDigit(value & 0x0f, 16));
        }
        return out.toString();
    }

    private static String cranposeReadText(InputStream input) throws IOException {
        try (InputStream stream = input; ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[8192];
            int read;
            while ((read = stream.read(buffer)) >= 0) {
                output.write(buffer, 0, read);
            }
            return output.toString("UTF-8");
        }
    }

    private static boolean cranposeIsNewerVersion(String latest, String current) {
        int[] remote = cranposeParseVersion(latest);
        int[] running = cranposeParseVersion(current);
        for (int index = 0; index < remote.length; index++) {
            if (remote[index] != running[index]) {
                return remote[index] > running[index];
            }
        }
        return false;
    }

    private static int[] cranposeParseVersion(String version) {
        int[] parts = new int[] {0, 0, 0};
        String[] values = version.trim().split("\\.");
        for (int index = 0; index < parts.length && index < values.length; index++) {
            String digits = values[index].replaceAll("[^0-9].*$", "");
            if (!digits.isEmpty()) {
                try {
                    parts[index] = Integer.parseInt(digits);
                } catch (NumberFormatException ignored) {
                }
            }
        }
        return parts;
    }

    private static void cranposeUpdateError(Throwable error) {
        String message = error.getMessage();
        nativeOnAppUpdateStatus(UPDATE_ERROR, "", "", 0, 0,
                message == null ? error.getClass().getSimpleName() : message.replace('\n', ' '), "");
    }

    /** Manifest meta-data key {@link NativeActivity} uses to name the native library. */
    private static final String NATIVE_LIB_NAME_META_DATA = "android.app.lib_name";

    /** Library name {@link NativeActivity} falls back to when the meta-data is absent. */
    private static final String DEFAULT_NATIVE_LIB_NAME = "main";

    private static final int REQUEST_BASE = 0x0C9A0000;
    private static final int REQUEST_CAMERA = REQUEST_BASE + 0x100;
    private static final int FLAG_FOLDER = 1;
    private static final int FLAG_WRITABLE = 4;
    private static final int FLAG_SAVE = 8;
    private static final int FLAG_MULTIPLE = 16;

    /** A fixed-name probe used by {@link #cranposeFolderWritable}; created and
     * deleted immediately, and ignored by listings. */
    private static final String WRITABLE_PROBE_NAME = ".cranpose-write-probe";

    private long pendingToken;
    private CranposeAccessibilityProvider cranposeAccessibilityProvider;
    private CranposeCamera cranposeCamera;
    private volatile CranposeMedia cranposeMedia;
    private boolean cranposeBackgroundActive;
    private boolean cranposePaused;

    public void cranposeSetBackgroundActive(boolean active) {
        cranposeBackgroundActive = active;
        runOnUiThread(() -> {
            if (active && cranposePaused) {
                startCranposeBackgroundService();
            } else if (!active) {
                stopService(new Intent(this, CranposeBackgroundService.class));
            }
        });
    }

    private void startCranposeBackgroundService() {
        Intent service = new Intent(this, CranposeBackgroundService.class);
        try {
            if (Build.VERSION.SDK_INT >= 26) {
                startForegroundService(service);
            } else {
                startService(service);
            }
        } catch (RuntimeException error) {
            android.util.Log.w("cranpose", "background work service could not start", error);
        }
    }

    private CranposeCamera cranposeCamera() {
        if (cranposeCamera == null) {
            cranposeCamera = new CranposeCamera(this);
        }
        return cranposeCamera;
    }

    public boolean cranposeCameraHasPermission() {
        return checkSelfPermission(android.Manifest.permission.CAMERA)
                == android.content.pm.PackageManager.PERMISSION_GRANTED;
    }

    public void cranposeCameraStart() {
        runOnUiThread(() -> {
            if (!cranposeCameraHasPermission()) {
                requestPermissions(new String[] {android.Manifest.permission.CAMERA}, REQUEST_CAMERA);
                return;
            }
            cranposeCamera().start();
        });
    }

    /** Asks for a still; it arrives through {@link #onCameraStill}. */
    public void cranposeCameraRequestStill() {
        runOnUiThread(() -> cranposeCamera().takeStill());
    }

    public void cranposeCameraStop() {
        runOnUiThread(() -> {
            if (cranposeCamera != null) {
                cranposeCamera.stop();
            }
        });
    }

    public String cranposeCameraLenses() {
        return cranposeCamera().lensList();
    }

    public String cranposeCameraLens() {
        return cranposeCamera().currentLens();
    }

    public boolean cranposeCameraUseLens(String id) {
        cranposeCamera().useLens(id);
        return id != null && id.equals(cranposeCamera().currentLens());
    }

    public boolean cranposeCameraHasFlash() {
        return cranposeCamera().hasFlash();
    }

    public boolean cranposeCameraSetFlash(int mode) {
        if (!cranposeCamera().hasFlash()) {
            return false;
        }
        cranposeCamera().setFlash(mode);
        return true;
    }

    /**
     * Synchronized because the transport calls in from the decode side as well
     * as from the UI thread: {@link #cranposeMediaRequestFocus()} answers on
     * whichever thread asked, and two of them must not each build a session.
     */
    private synchronized CranposeMedia cranposeMedia() {
        if (cranposeMedia == null) {
            cranposeMedia = new CranposeMedia(this);
        }
        return cranposeMedia;
    }

    /**
     * Takes audio focus for playback, reporting whether the broker granted it.
     * The decoder is Cranpose's own, so this is the whole of what Android is
     * asked for before a track starts.
     *
     * <p>Answered on the calling thread rather than posted: the caller needs
     * the verdict before it starts the stream.
     */
    public boolean cranposeMediaRequestFocus() {
        return cranposeMedia().requestFocus();
    }

    /** Gives audio focus back once nothing is playing. */
    public void cranposeMediaAbandonFocus() {
        runOnUiThread(() -> {
            if (cranposeMedia != null) {
                cranposeMedia.abandonFocus();
            }
        });
    }

    /**
     * Publishes where playback is to the lock screen; see the session constants
     * on {@link CranposeMedia}. A duration of {@code -1} means the item has
     * none.
     */
    public void cranposeMediaSessionUpdate(int state, long positionMs, long durationMs,
            float speed) {
        runOnUiThread(() -> cranposeMedia().sessionUpdate(state, positionMs, durationMs, speed));
    }

    public void cranposeMediaSetMetadata(String title, String artist) {
        runOnUiThread(() -> cranposeMedia().setMetadata(title, artist));
    }

    private static native void nativeOnAccessibilityActivate(float x, float y);

    private static native void nativeOnAccessibilityCustomAction(int virtualViewId, int actionIndex);

    /** Publishes Cranpose's semantic tree through Android's native virtual-view API. */
    public void cranposeSetAccessibilityElements(String payload) {
        final List<CranposeAccessibilityElement> elements = parseAccessibilityElements(payload);
        runOnUiThread(() -> {
            View host = getWindow().getDecorView();
            if (cranposeAccessibilityProvider == null) {
                cranposeAccessibilityProvider = new CranposeAccessibilityProvider(host);
                host.setFocusable(true);
                host.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_YES);
                host.setAccessibilityDelegate(new View.AccessibilityDelegate() {
                    @Override
                    public AccessibilityNodeProvider getAccessibilityNodeProvider(View ignored) {
                        return cranposeAccessibilityProvider;
                    }
                });
            }
            cranposeAccessibilityProvider.setElements(elements);
        });
    }

    /** Field count of one accessibility record; see android_accessibility_wire.rs. */
    private static final int ACCESSIBILITY_FIELDS = 17;

    /** Separator packing a node's custom action labels into one field. */
    private static final String ACCESSIBILITY_ACTION_SEPARATOR = String.valueOf((char) 0x1f);

    /**
     * First id handed to a custom action. Custom action ids only have to avoid
     * the framework's standard actions, which are bit flags well below this;
     * androidx solves the same problem by using R.id values, which live in the
     * app's 0x7f resource space, so this starts there too.
     */
    private static final int ACCESSIBILITY_CUSTOM_ACTION_BASE = 0x7f000000;

    private static List<CranposeAccessibilityElement> parseAccessibilityElements(String payload) {
        if (payload == null || payload.isEmpty()) return Collections.emptyList();
        ArrayList<CranposeAccessibilityElement> result = new ArrayList<>();
        for (String record : payload.split("\\n", -1)) {
            String[] fields = record.split("\\t", -1);
            if (fields.length != ACCESSIBILITY_FIELDS) continue;
            try {
                result.add(new CranposeAccessibilityElement(
                        Integer.parseInt(fields[0]), Integer.parseInt(fields[1]),
                        new Rect(Integer.parseInt(fields[2]), Integer.parseInt(fields[3]),
                                Integer.parseInt(fields[4]), Integer.parseInt(fields[5])),
                        Float.parseFloat(fields[6]), Float.parseFloat(fields[7]),
                        "1".equals(fields[8]), unescapeAccessibility(fields[9]),
                        unescapeAccessibility(fields[10]), unescapeAccessibility(fields[11]),
                        unescapeAccessibility(fields[12]), Integer.parseInt(fields[13]),
                        Integer.parseInt(fields[14]), "1".equals(fields[15]),
                        parseAccessibilityActions(fields[16])));
            } catch (RuntimeException ignored) {
                // A malformed record must not make the host Activity inaccessible.
            }
        }
        return result;
    }

    private static String[] parseAccessibilityActions(String field) {
        if (field.isEmpty()) return new String[0];
        String[] parts = field.split(ACCESSIBILITY_ACTION_SEPARATOR, -1);
        for (int i = 0; i < parts.length; i++) {
            parts[i] = unescapeAccessibility(parts[i]);
        }
        return parts;
    }

    private static String unescapeAccessibility(String value) {
        // %25 is undone last: it is the escape for the escape character, so
        // undoing it first would let an app-authored literal "%09" decode into
        // a tab.
        return value.replace("%0D", "\r").replace("%0A", "\n")
                .replace("%09", "\t").replace("%1F", ACCESSIBILITY_ACTION_SEPARATOR)
                .replace("%25", "%");
    }

    private static final class CranposeAccessibilityElement {
        final int id;
        final int role;
        final Rect bounds;
        final float centerX;
        final float centerY;
        final boolean clickable;
        final String label;
        final String value;
        final String stateDescription;
        final String clickLabel;
        /** -1 when the app said nothing, otherwise 0 or 1. */
        final int selected;
        final int toggled;
        final boolean enabled;
        final String[] customActions;

        CranposeAccessibilityElement(int id, int role, Rect bounds, float centerX,
                float centerY, boolean clickable, String label, String value,
                String stateDescription, String clickLabel, int selected, int toggled,
                boolean enabled, String[] customActions) {
            this.id = id;
            this.role = role;
            this.bounds = bounds;
            this.centerX = centerX;
            this.centerY = centerY;
            this.clickable = clickable;
            this.label = label;
            this.value = value;
            this.stateDescription = stateDescription;
            this.clickLabel = clickLabel;
            this.selected = selected;
            this.toggled = toggled;
            this.enabled = enabled;
            this.customActions = customActions;
        }

        /**
         * The widget class TalkBack reads the trailing noun from ("radio
         * button", "switch"). Cranpose's role codes are assigned in
         * android_accessibility_wire.rs.
         */
        String className() {
            switch (role) {
                case 1: return "android.widget.Button";
                case 3: return "android.widget.EditText";
                case 4: return "android.widget.CheckBox";
                case 5: return "android.widget.Switch";
                case 6: return "android.widget.RadioButton";
                case 7: return "android.widget.TabWidget";
                case 8: return "android.widget.ImageView";
                default: return "android.widget.TextView";
            }
        }

        /** Roles that TalkBack announces an on/off state for. */
        boolean isCheckable() {
            return role == 4 || role == 5;
        }
    }

    @SuppressWarnings("deprecation")
    private static final class CranposeAccessibilityProvider extends AccessibilityNodeProvider {
        private static final int HOST_ID = View.NO_ID;
        private final View host;
        private List<CranposeAccessibilityElement> elements = Collections.emptyList();
        private int focusedId = HOST_ID;

        CranposeAccessibilityProvider(View host) {
            this.host = host;
        }

        void setElements(List<CranposeAccessibilityElement> elements) {
            this.elements = elements;
            host.sendAccessibilityEvent(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
        }

        @Override
        public AccessibilityNodeInfo createAccessibilityNodeInfo(int virtualViewId) {
            if (virtualViewId == HOST_ID) {
                AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(host);
                info.setClassName(CranposeActivity.class.getName());
                info.setPackageName(host.getContext().getPackageName());
                for (CranposeAccessibilityElement element : elements) {
                    info.addChild(host, element.id);
                }
                return info;
            }
            CranposeAccessibilityElement element = find(virtualViewId);
            if (element == null) return null;
            AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain();
            info.setSource(host, element.id);
            info.setParent(host);
            info.setPackageName(host.getContext().getPackageName());
            info.setEnabled(element.enabled);
            info.setVisibleToUser(true);
            info.setFocusable(true);
            info.setAccessibilityFocused(focusedId == element.id);
            info.setContentDescription(element.label);
            info.setClassName(element.className());
            if (element.role == 3) {
                info.setEditable(true);
                info.setText(element.value);
            }
            if (element.role == 9 && Build.VERSION.SDK_INT >= 28) info.setHeading(true);
            // Compose's stateDescription. TalkBack speaks it after the label
            // and, unlike the label, re-speaks it on its own when only the
            // state changed — which is what makes a settings toggle usable.
            if (Build.VERSION.SDK_INT >= 30 && !element.stateDescription.isEmpty()) {
                info.setStateDescription(element.stateDescription);
            }
            if (element.isCheckable()) {
                info.setCheckable(true);
                info.setChecked(element.toggled == 1);
            }
            if (element.selected >= 0) info.setSelected(element.selected == 1);
            info.setBoundsInParent(element.bounds);
            int[] location = new int[2];
            host.getLocationOnScreen(location);
            Rect screen = new Rect(element.bounds);
            screen.offset(location[0], location[1]);
            info.setBoundsInScreen(screen);
            info.setClickable(element.clickable);
            if (element.clickable) {
                // A labelled click is Compose's onClick(label = …): TalkBack
                // reads "double tap to <label>" instead of the generic
                // "double tap to activate".
                if (element.clickLabel.isEmpty()) {
                    info.addAction(AccessibilityNodeInfo.ACTION_CLICK);
                } else {
                    info.addAction(new AccessibilityNodeInfo.AccessibilityAction(
                            AccessibilityNodeInfo.ACTION_CLICK, element.clickLabel));
                }
            }
            for (int i = 0; i < element.customActions.length; i++) {
                info.addAction(new AccessibilityNodeInfo.AccessibilityAction(
                        ACCESSIBILITY_CUSTOM_ACTION_BASE + i, element.customActions[i]));
            }
            info.addAction(focusedId == element.id
                    ? AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS
                    : AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS);
            return info;
        }

        @Override
        public boolean performAction(int virtualViewId, int action, Bundle arguments) {
            CranposeAccessibilityElement element = find(virtualViewId);
            if (element == null) return false;
            if (action == AccessibilityNodeInfo.ACTION_CLICK && element.clickable) {
                nativeOnAccessibilityActivate(element.centerX, element.centerY);
                sendEvent(element.id, AccessibilityEvent.TYPE_VIEW_CLICKED);
                return true;
            }
            int customIndex = action - ACCESSIBILITY_CUSTOM_ACTION_BASE;
            if (customIndex >= 0 && customIndex < element.customActions.length) {
                // Only the identity crosses back; the handler is resolved on
                // the frame loop against the live semantics tree, so a stale
                // published snapshot cannot run a stale closure.
                nativeOnAccessibilityCustomAction(element.id, customIndex);
                return true;
            }
            if (action == AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS) {
                focusedId = element.id;
                sendEvent(element.id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED);
                return true;
            }
            if (action == AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS) {
                focusedId = HOST_ID;
                sendEvent(element.id, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED);
                return true;
            }
            return false;
        }

        private CranposeAccessibilityElement find(int id) {
            for (CranposeAccessibilityElement element : elements) {
                if (element.id == id) return element;
            }
            return null;
        }

        private void sendEvent(int id, int type) {
            if (!host.isShown()) return;
            AccessibilityEvent event = AccessibilityEvent.obtain(type);
            event.setPackageName(host.getContext().getPackageName());
            event.setSource(host, id);
            host.getParent().requestSendAccessibilityEvent(host, event);
        }
    }

    /** Number of files to accumulate before flushing a streaming batch. */
    private static final int FOLDER_BATCH_SIZE = 32;

    /** How many times to try listing a folder before skipping it (slow cloud /
     * WebDAV shares fail transiently). */
    private static final int FOLDER_QUERY_ATTEMPTS = 4;

    /** Base backoff between folder-listing retries, in ms (grows per attempt). */
    private static final int FOLDER_QUERY_RETRY_MS = 250;

    /** How many times to re-query a folder whose cursor reports
     * {@link DocumentsContract#EXTRA_LOADING} before giving up and using
     * whatever it returns (a network provider keeps the listing "loading" while
     * it fetches over the wire). */
    private static final int FOLDER_LOADING_ATTEMPTS = 24;

    /** Delay between {@link DocumentsContract#EXTRA_LOADING} re-queries, in ms
     * ({@link #FOLDER_LOADING_ATTEMPTS} × this ≈ the time budget for one folder
     * to finish loading). */
    private static final int FOLDER_LOADING_POLL_MS = 250;

    /** Records a granted selection in the native, process-static "resume inbox"
     * so a pick whose result arrives after the requesting activity (and the
     * native app) was destroyed is not lost. Android destroys and recreates the
     * activity when the SAF picker covers it on some devices, tearing down the
     * composition that was awaiting the result; the app drains this inbox on its
     * next start to recover the selection. {@code flags} are the request flags
     * ({@link #FLAG_FOLDER}/{@link #FLAG_MULTIPLE}/{@link #FLAG_WRITABLE}).
     * Implemented in the cdylib. */
    private static native void nativeRecordResumablePick(int flags, String entries);

    /** Implemented in the Rust cdylib. {@code entries} is newline-separated
     * {@code uri\tname\tmime\tsize\tmodified} rows — one per chosen document. */
    private static native void nativeOnFilePicked(
            long token, String entries, boolean cancelled, String error);

    /** A folder was granted (or cancelled/failed). {@code uri} is the tree URI. */
    private static native void nativeOnFolderPicked(
            long token, String uri, boolean cancelled, String error);

    /** A save destination was created (or cancelled/failed). {@code uri} is the
     * new document's URI, which the caller streams into. */
    private static native void nativeOnDocumentCreated(
            long token, String uri, boolean cancelled, String error);

    /** A streaming batch of discovered files ({@code uri\tname} rows). Returns
     * {@code false} once the consumer has gone away, so enumeration can stop. */
    private static native boolean nativeOnFolderEntries(long token, String entries);

    /** Folder enumeration finished, with an optional error. */
    private static native void nativeOnFolderFinished(long token, String error);

    /** A writable folder was picked (or cancelled/failed). {@code uri} is the
     * persisted SAF tree URI on success. */
    private static native void nativeOnWritableFolderPicked(
            long token, String uri, boolean cancelled, String error);

    /** Opens the document picker for a single file. {@code mimeTypes} is a
     * newline-separated filter list, empty for any type. Called from Rust over
     * JNI. */
    public void cranposePickFile(long token, String mimeTypes) {
        pendingToken = token;
        launch(openDocumentIntent(mimeTypes), token, 0);
    }

    /** Opens the document picker for several files at once. Called from Rust
     * over JNI. */
    public void cranposePickFiles(long token, String mimeTypes) {
        pendingToken = token;
        Intent intent = openDocumentIntent(mimeTypes);
        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        launch(intent, token, FLAG_MULTIPLE);
    }

    private Intent openDocumentIntent(String mimeTypes) {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        String[] types = splitRows(mimeTypes);
        if (types.length == 1) {
            intent.setType(types[0]);
        } else if (types.length > 1) {
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_MIME_TYPES, types);
        } else {
            intent.setType("*/*");
        }
        return intent;
    }

    private static String[] splitRows(String rows) {
        if (rows == null || rows.isEmpty()) {
            return new String[0];
        }
        return rows.split("\n");
    }

    /** Opens the document tree picker for a folder. The granted tree URI comes
     * back through {@link #nativeOnFolderPicked}; enumeration is a separate
     * request. Called from Rust over JNI. */
    public void cranposePickFolder(long token) {
        pendingToken = token;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        launch(intent, token, FLAG_FOLDER);
    }

    /** Streams every file under an already-granted tree URI. Called from Rust
     * over JNI once the application collects the folder's files. */
    public void cranposeStreamFolder(long token, String uriString) {
        final Uri uri = Uri.parse(uriString);
        new Thread(() -> {
            String error = null;
            try {
                streamTree(uri, token);
            } catch (Exception failure) {
                error = failure.toString();
            }
            nativeOnFolderFinished(token, error);
        }, "cranpose-folder-stream").start();
    }

    /** Lists the immediate children of a granted tree (or of one of its
     * sub-documents) as {@code uri\tname\tmime\tsize\tmodified} rows.
     * Directories carry the {@code vnd.android.document/directory} MIME type so
     * the caller can tell them apart. Returns {@code null} on a read failure.
     * Called from Rust over JNI. */
    public String cranposeFolderChildren(String treeUriString, String documentId) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String docId = documentId == null || documentId.isEmpty()
                    ? DocumentsContract.getTreeDocumentId(tree)
                    : documentId;
            Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId);
            Cursor cursor = queryChildrenWithRetry(childrenUri);
            if (cursor == null) {
                return null;
            }
            StringBuilder out = new StringBuilder();
            try {
                while (cursor.moveToNext()) {
                    Uri childUri =
                            DocumentsContract.buildDocumentUriUsingTree(tree, cursor.getString(0));
                    appendDocumentRow(out, childUri, cursor);
                }
            } finally {
                cursor.close();
            }
            return out.toString();
        } catch (Exception error) {
            return null;
        }
    }

    /** Opens the document tree picker for a <em>writable</em> folder, taking a
     * persistent read/write grant. The chosen tree URI is reported back through
     * {@link #nativeOnWritableFolderPicked}. Called from Rust over JNI. */
    @SuppressWarnings("deprecation")
    public void cranposePickWritableFolder(long token) {
        pendingToken = token;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        try {
            startActivityForResult(intent, REQUEST_BASE | FLAG_WRITABLE);
        } catch (Exception error) {
            nativeOnWritableFolderPicked(token, null, false, error.toString());
        }
    }

    /**
     * Opens a picked {@code content://} document for reading and returns a
     * detached file descriptor. The Rust caller owns and closes it. Called over
     * JNI when a track is played, so nothing is copied up front.
     */
    public int cranposeOpenUri(String uriString) throws IOException {
        ParcelFileDescriptor descriptor =
                getContentResolver().openFileDescriptor(Uri.parse(uriString), "r");
        if (descriptor == null) {
            throw new IOException("no descriptor for " + uriString);
        }
        return descriptor.detachFd();
    }

    /**
     * How many bytes the document at {@code uriString} is, or {@code -1} where
     * the provider does not say.
     *
     * <p>The provider's own answer, not the descriptor's: a provider that
     * fetches its bytes over a network hands back a pipe, which cannot be
     * stat-ed, while still listing a size for the document. A decoder that
     * knows the length can seek by it instead of reading to the end to find out
     * where the end is.
     */
    public long cranposeContentLength(String uriString) {
        try (Cursor cursor = getContentResolver().query(
                Uri.parse(uriString),
                new String[] {OpenableColumns.SIZE},
                null,
                null,
                null)) {
            if (cursor == null || !cursor.moveToFirst()) {
                return -1;
            }
            int column = cursor.getColumnIndex(OpenableColumns.SIZE);
            if (column < 0 || cursor.isNull(column)) {
                return -1;
            }
            long size = cursor.getLong(column);
            return size > 0 ? size : -1;
        } catch (Exception error) {
            return -1;
        }
    }

    /**
     * Opens a {@code content://} document for writing, truncating it first, and
     * returns a detached file descriptor the Rust caller owns and closes. Used
     * to stream a saved document instead of buffering it.
     */
    public int cranposeOpenUriWrite(String uriString) throws IOException {
        ParcelFileDescriptor descriptor =
                getContentResolver().openFileDescriptor(Uri.parse(uriString), "rwt");
        if (descriptor == null) {
            throw new IOException("no writable descriptor for " + uriString);
        }
        return descriptor.detachFd();
    }

    /** Metadata for one document as {@code name\tmime\tsize\tmodified}, or
     * {@code null} when the provider cannot describe it. */
    public String cranposeDocumentInfo(String uriString) {
        Uri uri = Uri.parse(uriString);
        String[] columns = {
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED,
        };
        try (Cursor cursor = getContentResolver().query(uri, columns, null, null, null)) {
            if (cursor == null || !cursor.moveToFirst()) {
                return null;
            }
            return sanitize(cursor.isNull(0) ? "" : cursor.getString(0))
                    + '\t' + (cursor.isNull(1) ? "" : cursor.getString(1))
                    + '\t' + (cursor.isNull(2) ? "" : Long.toString(cursor.getLong(2)))
                    + '\t' + (cursor.isNull(3) ? "" : Long.toString(cursor.getLong(3)));
        } catch (Exception error) {
            return null;
        }
    }

    /** Writes (overwriting) {@code contents} to a file named {@code name} in the
     * writable tree. Returns 0 on success, 1 on permission failure (read-only),
     * 2 on any other error. Called from the Rust sync worker thread over JNI. */
    public int cranposeFolderWrite(String treeUriString, String name, byte[] contents) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri parent = DocumentsContract.buildDocumentUriUsingTree(tree, treeDocId);
            String docId = findWritableChildId(tree, treeDocId, name);
            Uri docUri;
            if (docId != null) {
                docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            } else {
                docUri = createDocumentWithRetry(parent, name);
                if (docUri == null) {
                    return 2;
                }
            }
            try (OutputStream output = getContentResolver().openOutputStream(docUri, "wt")) {
                if (output == null) {
                    return 2;
                }
                output.write(contents);
            }
            return 0;
        } catch (SecurityException error) {
            return 1;
        } catch (Exception error) {
            return 2;
        }
    }

    /** Lists immediate child files (directories excluded) as
     * {@code name\tsize\tmodified} rows, newline-joined. Returns {@code null}
     * only on a hard read failure (an empty folder is ""). */
    public String cranposeFolderList(String treeUriString) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, treeDocId);
            Cursor cursor = queryChildrenWithRetry(childrenUri);
            if (cursor == null) {
                return null;
            }
            StringBuilder out = new StringBuilder();
            try {
                while (cursor.moveToNext()) {
                    String name = cursor.getString(1);
                    String mime = cursor.getString(2);
                    if (name == null || DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                        continue;
                    }
                    if (out.length() > 0) {
                        out.append('\n');
                    }
                    out.append(sanitize(name))
                            .append('\t').append(cursor.isNull(3) ? "" : Long.toString(cursor.getLong(3)))
                            .append('\t').append(cursor.isNull(4) ? "" : Long.toString(cursor.getLong(4)));
                }
            } finally {
                cursor.close();
            }
            return out.toString();
        } catch (Exception error) {
            return null;
        }
    }

    /** Opens a child of the writable tree for reading and returns a detached
     * descriptor, or -1 when the file is absent or unreadable. Called from the
     * Rust worker thread over JNI so a large file streams instead of being
     * buffered. */
    public int cranposeFolderOpenRead(String treeUriString, String name) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String docId = findWritableChildId(tree, DocumentsContract.getTreeDocumentId(tree), name);
            if (docId == null) {
                return -1;
            }
            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            ParcelFileDescriptor descriptor =
                    getContentResolver().openFileDescriptor(docUri, "r");
            return descriptor == null ? -1 : descriptor.detachFd();
        } catch (Exception error) {
            return -1;
        }
    }

    /** Creates (or truncates) a child of the writable tree and returns a
     * detached write descriptor, or -1 on failure. Callers stage under a
     * temporary name and commit with {@link #cranposeFolderCommit}. */
    public int cranposeFolderOpenWrite(String treeUriString, String name) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri parent = DocumentsContract.buildDocumentUriUsingTree(tree, treeDocId);
            String docId = findWritableChildId(tree, treeDocId, name);
            Uri docUri = docId != null
                    ? DocumentsContract.buildDocumentUriUsingTree(tree, docId)
                    : createDocumentWithRetry(parent, name);
            if (docUri == null) {
                return -1;
            }
            ParcelFileDescriptor descriptor =
                    getContentResolver().openFileDescriptor(docUri, "rwt");
            return descriptor == null ? -1 : descriptor.detachFd();
        } catch (Exception error) {
            return -1;
        }
    }

    /** Replaces {@code finalName} with the staged {@code stagingName}. Returns 0
     * on success, 1 when the tree is read-only, 2 otherwise. */
    public int cranposeFolderCommit(String treeUriString, String stagingName, String finalName) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            String stagingId = findWritableChildId(tree, treeDocId, stagingName);
            if (stagingId == null) {
                return 2;
            }
            String existingId = findWritableChildId(tree, treeDocId, finalName);
            if (existingId != null) {
                DocumentsContract.deleteDocument(getContentResolver(),
                        DocumentsContract.buildDocumentUriUsingTree(tree, existingId));
            }
            Uri staged = DocumentsContract.buildDocumentUriUsingTree(tree, stagingId);
            return DocumentsContract.renameDocument(getContentResolver(), staged, finalName) == null
                    ? 2
                    : 0;
        } catch (SecurityException error) {
            return 1;
        } catch (Exception error) {
            return 2;
        }
    }

    /** Reads the file {@code name} as bytes, or {@code null} if absent/unreadable. */
    public byte[] cranposeFolderRead(String treeUriString, String name) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            String docId = findWritableChildId(tree, treeDocId, name);
            if (docId == null) {
                return null;
            }
            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            try (InputStream input = getContentResolver().openInputStream(docUri)) {
                if (input == null) {
                    return null;
                }
                ByteArrayOutputStream buffer = new ByteArrayOutputStream();
                byte[] chunk = new byte[8192];
                int read;
                while ((read = input.read(chunk)) >= 0) {
                    buffer.write(chunk, 0, read);
                }
                return buffer.toByteArray();
            }
        } catch (Exception error) {
            return null;
        }
    }

    /** Deletes the file {@code name}. Returns 0 on success (or already gone), 2 otherwise. */
    public int cranposeFolderRemove(String treeUriString, String name) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            String docId = findWritableChildId(tree, treeDocId, name);
            if (docId == null) {
                return 0;
            }
            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            return DocumentsContract.deleteDocument(getContentResolver(), docUri) ? 0 : 2;
        } catch (Exception error) {
            return 2;
        }
    }

    /** Whether the tree is writable now: a persisted write grant exists AND a
     * probe document can be created and deleted (catching a read-only backing
     * store such as a read-only WebDAV mount). */
    public boolean cranposeFolderWritable(String treeUriString) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri parent = DocumentsContract.buildDocumentUriUsingTree(tree, treeDocId);
            // Ground truth: actually create (then delete) a probe document. We do
            // NOT pre-gate on UriPermission.isWritePermission(): some third-party
            // document providers (rclone / WebDAV via RoundSync) grant working
            // write access while the persisted permission reads back WITHOUT the
            // write flag, which produced false "read-only" results on folders the
            // user can demonstrably write to. createDocumentWithRetry rides out a
            // cold/slow network provider; a genuinely read-only folder throws
            // SecurityException and returns null promptly.
            String existing = findWritableChildId(tree, treeDocId, WRITABLE_PROBE_NAME);
            if (existing != null) {
                DocumentsContract.deleteDocument(
                        getContentResolver(),
                        DocumentsContract.buildDocumentUriUsingTree(tree, existing));
            }
            Uri probe = createDocumentWithRetry(parent, WRITABLE_PROBE_NAME);
            if (probe == null) {
                return false;
            }
            DocumentsContract.deleteDocument(getContentResolver(), probe);
            return true;
        } catch (Exception error) {
            return false;
        }
    }

    /** Creates a document, retrying a slow/flaky network provider (a WebDAV /
     * cloud share mounted through RoundSync can transiently return {@code null}
     * or throw before it settles, just like a folder listing). Returns the new
     * document URI, or {@code null} if it never succeeded. A
     * {@link SecurityException} means the tree is genuinely read-only, so we stop
     * immediately rather than retry. */
    private Uri createDocumentWithRetry(Uri parent, String name) {
        for (int attempt = 1; ; attempt++) {
            try {
                Uri doc = DocumentsContract.createDocument(
                        getContentResolver(), parent, "application/octet-stream", name);
                if (doc != null) {
                    return doc;
                }
            } catch (SecurityException readOnly) {
                return null;
            } catch (Exception transientError) {
                // Slow/flaky provider — fall through and retry.
            }
            if (attempt >= FOLDER_QUERY_ATTEMPTS
                    || !sleepQuietly((long) FOLDER_QUERY_RETRY_MS * attempt)) {
                return null;
            }
        }
    }

    /** Finds the document id of a direct child by display name, or {@code null}. */
    private String findWritableChildId(Uri treeUri, String treeDocId, String name) {
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, treeDocId);
        Cursor cursor = queryChildrenWithRetry(childrenUri);
        if (cursor == null) {
            return null;
        }
        try {
            while (cursor.moveToNext()) {
                if (name.equals(cursor.getString(1))) {
                    return cursor.getString(0);
                }
            }
        } finally {
            cursor.close();
        }
        return null;
    }

    @SuppressWarnings("deprecation")
    private void launch(Intent intent, long token, int flags) {
        try {
            startActivityForResult(intent, REQUEST_BASE | flags);
        } catch (Exception error) {
            if ((flags & FLAG_FOLDER) != 0) {
                nativeOnFolderPicked(token, null, false, error.toString());
            } else if ((flags & FLAG_SAVE) != 0) {
                nativeOnDocumentCreated(token, null, false, error.toString());
            } else {
                nativeOnFilePicked(token, null, false, error.toString());
            }
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if ((requestCode & 0xFFFF0000) != REQUEST_BASE) {
            return;
        }
        final long token = pendingToken;
        final int flags = requestCode & 0x0000FFFF;
        final boolean ok = resultCode == RESULT_OK && data != null;
        final Uri primary = ok ? data.getData() : null;

        if ((flags & FLAG_SAVE) != 0) {
            if (primary == null) {
                nativeOnDocumentCreated(token, null, true, null);
            } else {
                nativeOnDocumentCreated(token, primary.toString(), false, null);
            }
            return;
        }

        if ((flags & FLAG_WRITABLE) != 0) {
            if (primary == null) {
                nativeOnWritableFolderPicked(token, null, true, null);
                return;
            }
            try {
                getContentResolver().takePersistableUriPermission(primary,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION
                                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
                // Record the grant in the resume inbox before delivering it, so a
                // pick whose activity was recreated mid-prompt (tearing down the
                // awaiting composition) can still be reclaimed on the next start.
                // Live delivery below clears the inbox on the happy path.
                nativeRecordResumablePick(flags, primary.toString());
                nativeOnWritableFolderPicked(token, primary.toString(), false, null);
            } catch (Exception error) {
                nativeOnWritableFolderPicked(token, null, false, error.toString());
            }
            return;
        }

        if ((flags & FLAG_FOLDER) != 0) {
            if (primary == null) {
                nativeOnFolderPicked(token, null, true, null);
                return;
            }
            try {
                getContentResolver().takePersistableUriPermission(
                        primary, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (SecurityException ignored) {
            }
            // Record the grant *before* delivering it, so that if this activity
            // was recreated (the SAF picker covered and destroyed it, tearing down
            // the awaiting composition) the next app start can still reclaim it.
            // Live delivery clears the inbox on the happy path.
            nativeRecordResumablePick(flags, primary.toString());
            nativeOnFolderPicked(token, primary.toString(), false, null);
            return;
        }

        final List<Uri> documents = pickedDocuments(data, primary);
        if (documents.isEmpty()) {
            nativeOnFilePicked(token, null, true, null);
            return;
        }
        // Describing documents only reads provider metadata, but a slow provider
        // can still take a moment, so never do it on the UI thread.
        new Thread(() -> {
            try {
                String entries = describeDocuments(documents);
                // A single- or multi-file pick can also be lost to an activity
                // recreation, so record it for resume before delivering.
                nativeRecordResumablePick(flags, entries);
                nativeOnFilePicked(token, entries, false, null);
            } catch (Exception error) {
                nativeOnFilePicked(token, null, false, error.toString());
            }
        }, "cranpose-file-picker").start();
    }

    /** Every document the user chose: the {@code ClipData} items of a
     * multi-selection, or the single {@code getData()} URI. */
    private static List<Uri> pickedDocuments(Intent data, Uri primary) {
        List<Uri> documents = new ArrayList<>();
        ClipData clip = data == null ? null : data.getClipData();
        if (clip != null) {
            for (int index = 0; index < clip.getItemCount(); index++) {
                Uri uri = clip.getItemAt(index).getUri();
                if (uri != null) {
                    documents.add(uri);
                }
            }
        }
        if (documents.isEmpty() && primary != null) {
            documents.add(primary);
        }
        return documents;
    }

    private String describeDocuments(List<Uri> documents) {
        StringBuilder out = new StringBuilder();
        for (Uri uri : documents) {
            if (out.length() > 0) {
                out.append('\n');
            }
            String info = cranposeDocumentInfo(uri.toString());
            out.append(uri.toString()).append('\t')
                    .append(info == null
                            ? sanitize(displayName(uri, "file")) + "\t\t\t"
                            : info);
        }
        return out.toString();
    }

    private String describeFile(Uri uri) {
        return uri.toString() + "\t" + sanitize(displayName(uri, "file"));
    }

    /** Walks {@code treeUri} depth-first, flushing files to Rust in batches as
     * they are found. Stops early if the consumer drops the stream. */
    private void streamTree(Uri treeUri, long token) {
        try {
            getContentResolver().takePersistableUriPermission(
                    treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
        } catch (SecurityException ignored) {
        }
        Batch batch = new Batch(token);
        collectTreeStreaming(treeUri, DocumentsContract.getTreeDocumentId(treeUri), batch);
        batch.flush();
    }

    /** Returns {@code false} once the consumer has gone away, so recursion can
     * unwind without finishing the walk. */
    private boolean collectTreeStreaming(Uri treeUri, String documentId, Batch batch) {
        if (batch.stopped()) {
            return false;
        }
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId);
        // Query this folder with retries. A slow document provider (a mounted
        // WebDAV / cloud share) transiently fails requests; without this, one
        // failed query would throw and abort the ENTIRE walk, so a big library
        // over a flaky link could end up adding nothing. On persistent failure we
        // skip just this folder and let the rest of the tree keep streaming in.
        Cursor cursor = queryChildrenWithRetry(childrenUri);
        if (cursor == null) {
            return !batch.stopped();
        }
        try {
            while (cursor.moveToNext()) {
                if (batch.stopped()) {
                    return false;
                }
                String childId = cursor.getString(0);
                String mime = cursor.getString(2);
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                    if (!collectTreeStreaming(treeUri, childId, batch)) {
                        return false;
                    }
                } else {
                    Uri childUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, childId);
                    batch.add(childUri, cursor);
                }
            }
        } finally {
            cursor.close();
        }
        return !batch.stopped();
    }

    /** Lists a folder's children, retrying transient provider failures (a slow
     * cloud/WebDAV share throws intermittently). Returns {@code null} if the
     * folder cannot be read after {@link #FOLDER_QUERY_ATTEMPTS} tries, so the
     * caller skips just that folder instead of aborting the whole walk. */
    private Cursor queryChildrenWithRetry(Uri childrenUri) {
        String[] columns = {
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED,
        };
        int errorAttempt = 0;
        int loadingAttempt = 0;
        while (true) {
            Cursor cursor;
            try {
                cursor = getContentResolver().query(childrenUri, columns, null, null, null);
            } catch (Exception error) {
                errorAttempt++;
                if (errorAttempt >= FOLDER_QUERY_ATTEMPTS) {
                    android.util.Log.w(
                            "Cranpose",
                            "skipping unreadable folder after " + errorAttempt + " tries: "
                                    + childrenUri + " (" + error + ")");
                    return null;
                }
                if (!sleepQuietly((long) FOLDER_QUERY_RETRY_MS * errorAttempt)) {
                    return null;
                }
                continue;
            }
            if (cursor == null) {
                return null;
            }
            // A slow network document provider (RoundSync/rclone, a mounted WebDAV
            // share) returns an EMPTY cursor immediately with EXTRA_LOADING=true
            // while it fetches the real listing over the wire, then notifies and
            // serves the cached result on the next query. Reading the cursor right
            // now yields zero children, so the folder — even the picked root — looks
            // empty ("adds nothing"). Re-query until it stops loading (or we run out
            // of patience) instead of trusting the placeholder.
            if (isLoading(cursor) && loadingAttempt < FOLDER_LOADING_ATTEMPTS) {
                loadingAttempt++;
                cursor.close();
                if (!sleepQuietly(FOLDER_LOADING_POLL_MS)) {
                    return null;
                }
                continue;
            }
            return cursor;
        }
    }

    /** True if the provider flagged this cursor as still fetching its results. */
    private static boolean isLoading(Cursor cursor) {
        Bundle extras = cursor.getExtras();
        return extras != null && extras.getBoolean(DocumentsContract.EXTRA_LOADING, false);
    }

    /** Sleeps, returning {@code false} (so callers can bail) if interrupted. */
    private static boolean sleepQuietly(long millis) {
        try {
            Thread.sleep(millis);
            return true;
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return false;
        }
    }

    /** Accumulates {@code uri\tname} rows and flushes them to Rust every
     * {@link #FOLDER_BATCH_SIZE} files, recording when the consumer stops. */
    private final class Batch {
        private final long token;
        private final StringBuilder rows = new StringBuilder();
        private int count;
        private boolean stopped;

        Batch(long token) {
            this.token = token;
        }

        void add(Uri uri, Cursor cursor) {
            if (stopped) {
                return;
            }
            appendDocumentRow(rows, uri, cursor);
            count++;
            if (count >= FOLDER_BATCH_SIZE) {
                flush();
            }
        }

        void flush() {
            if (stopped || rows.length() == 0) {
                return;
            }
            boolean keepGoing = nativeOnFolderEntries(token, rows.toString());
            rows.setLength(0);
            count = 0;
            if (!keepGoing) {
                stopped = true;
            }
        }

        boolean stopped() {
            return stopped;
        }
    }

    /** Appends one {@code uri\tname\tmime\tsize\tmodified} row read from a
     * children cursor positioned on the document. The projection is the one
     * {@link #queryChildrenWithRetry} requests. */
    private void appendDocumentRow(StringBuilder out, Uri uri, Cursor cursor) {
        if (out.length() > 0) {
            out.append('\n');
        }
        out.append(uri.toString())
                .append('\t').append(sanitize(cursor.isNull(1) ? "" : cursor.getString(1)))
                .append('\t').append(cursor.isNull(2) ? "" : cursor.getString(2))
                .append('\t').append(cursor.isNull(3) ? "" : Long.toString(cursor.getLong(3)))
                .append('\t').append(cursor.isNull(4) ? "" : Long.toString(cursor.getLong(4)));
    }

    private String displayName(Uri uri, String fallback) {
        try (Cursor cursor = getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    String name = cursor.getString(column);
                    if (name != null && !name.isEmpty()) {
                        return name;
                    }
                }
            }
        } catch (Exception ignored) {
        }
        return fallback;
    }

    /** The transport joins rows with newlines and fields with tabs, so strip
     * those from display names. */
    private String sanitize(String name) {
        if (name == null) {
            return "";
        }
        return name.replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
    }

    // ------------------------------------------------------------------
    // Platform capabilities (share, notifications, clipboard, haptics,
    // network status, window insets, save-file) — the Android backends of
    // cranpose_services. Rust calls the cranpose* methods over JNI; the
    // native* methods push events back into the cdylib.
    // ------------------------------------------------------------------

    /** Cranpose's single notification channel. */
    private static final String NOTIFY_CHANNEL = "cranpose";
    /** Launch-intent extra carrying a notification deep-link payload. */
    private static final String EXTRA_DEEPLINK = "cranpose_deeplink";
    private static final int REQ_NOTIFY_PERMISSION = 0x0C9B;

    /** Bytes staged for an in-flight ACTION_CREATE_DOCUMENT save. */
    private ConnectivityManager.NetworkCallback cranposeNetworkCallback;

    /** Current network state (online, metered) changed. */
    private static native void nativeOnNetworkStatus(boolean online, boolean metered);
    private static native void nativeOnNetworkRegistration(boolean registered);

    /** System-bar/cutout insets changed (physical px). */
    private static native void nativeOnInsetsChanged(int left, int top, int right, int bottom);

    /** The user tapped a notification carrying a deep-link. */
    private static native void nativeNotificationAction(String deeplink);

    /** A new launching intent replaced the old one; carries re-encoded extras. */
    private static native void nativeOnLaunchArguments(String payload);

    /**
     * Loads the app's native library into this class loader so that the {@code native}
     * methods declared above can resolve.
     *
     * <p>{@link NativeActivity} loads the library itself, but through libnativeloader's
     * {@code OpenNativeLibrary}, which never registers it with ART's JNI method resolver.
     * The native entry point still runs, so the app appears to start, and then the first
     * synchronous Java-to-native call dies with {@code UnsatisfiedLinkError} even though
     * the symbol is present in the packaged {@code .so}. Loading it here, before
     * {@code super.onCreate}, is what makes the symbols resolvable from Java.
     *
     * <p>The library name comes from the same {@code android.app.lib_name} manifest
     * meta-data that {@link NativeActivity} reads, so subclasses need no extra wiring.
     */
    private void loadCranposeNativeLibrary() {
        String libraryName = DEFAULT_NATIVE_LIB_NAME;
        try {
            android.content.pm.ActivityInfo info =
                    getPackageManager()
                            .getActivityInfo(
                                    getComponentName(),
                                    android.content.pm.PackageManager.GET_META_DATA);
            if (info.metaData != null) {
                String declared = info.metaData.getString(NATIVE_LIB_NAME_META_DATA);
                if (declared != null && !declared.isEmpty()) {
                    libraryName = declared;
                }
            }
        } catch (android.content.pm.PackageManager.NameNotFoundException error) {
            android.util.Log.w("cranpose", "activity info unavailable; using default lib name", error);
        }
        try {
            System.loadLibrary(libraryName);
        } catch (UnsatisfiedLinkError error) {
            android.util.Log.w("cranpose", "could not load native library " + libraryName, error);
        }
    }

    /**
     * Makes the native content view focusable and gives it focus.
     *
     * <p>Wear OS delivers rotary encoder events — the Pixel Watch crown, the Galaxy
     * Watch bezel — only to a focused view. {@link NativeActivity} never marks its
     * content view focusable, so without this the crown produces nothing at all, with
     * no error to explain the silence. Touch is unaffected, which makes it look like
     * the app simply ignores the crown.
     *
     * <p>Re-applied from {@code onWindowFocusChanged} because the window can hand
     * focus elsewhere (dialogs, IME) and not give it back to the content view.
     */
    private void focusNativeContentView() {
        View content = findViewById(android.R.id.content);
        if (content instanceof android.view.ViewGroup) {
            android.view.ViewGroup group = (android.view.ViewGroup) content;
            if (group.getChildCount() > 0) {
                content = group.getChildAt(0);
            }
        }
        if (content == null) {
            return;
        }
        content.setFocusable(true);
        content.setFocusableInTouchMode(true);
        content.requestFocus();
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        loadCranposeNativeLibrary();
        super.onCreate(savedInstanceState);
        focusNativeContentView();
        installInsetsListener();
        registerNetworkCallback();
        dispatchDeeplink(getIntent());
        dispatchIncomingShares(getIntent());
        registerBackCallback();
        IntentFilter updateFilter = new IntentFilter(cranposeUpdateInstallAction());
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(
                    cranposeUpdateInstallReceiver, updateFilter, Context.RECEIVER_NOT_EXPORTED);
        } else {
            registerReceiver(cranposeUpdateInstallReceiver, updateFilter);
        }
    }

    private void registerBackCallback() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            return;
        }
        OnBackInvokedCallback callback = () -> {
            if (!nativeOnBackInvoked()) {
                finish();
            }
        };
        getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            focusNativeContentView();
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        dispatchDeeplink(intent);
        dispatchIncomingShares(intent);
        // setIntent above is what makes getIntent() — and therefore the encoder —
        // report the new extras, mirroring what a Compose activity sees.
        nativeOnLaunchArguments(cranposeEncodeLaunchArguments());
    }

    private void dispatchIncomingShares(Intent intent) {
        if (intent == null || (!Intent.ACTION_SEND.equals(intent.getAction())
                && !Intent.ACTION_SEND_MULTIPLE.equals(intent.getAction()))) {
            return;
        }
        ArrayList<Uri> uris = new ArrayList<>();
        if (Intent.ACTION_SEND.equals(intent.getAction())) {
            Uri uri = incomingStream(intent);
            if (uri != null) {
                uris.add(uri);
            }
        } else {
            ArrayList<Uri> shared = incomingStreams(intent);
            if (shared != null) {
                uris.addAll(shared);
            }
        }
        String mimeType = intent.getType() == null ? "" : intent.getType();
        android.util.Log.i("cranpose", "incoming share: " + uris.size()
                + " uri(s), type " + mimeType);
        // The URI is published, not the bytes: the framework opens it through the
        // content resolver when the application actually reads it, so sharing a
        // multi-gigabyte video costs nothing until it is used.
        new Thread(() -> {
            for (Uri uri : uris) {
                if (!"content".equalsIgnoreCase(uri.getScheme())) {
                    android.util.Log.i("cranpose",
                            "incoming share: skipped, scheme " + uri.getScheme());
                    continue;
                }
                try {
                    android.content.pm.ProviderInfo provider =
                            getPackageManager().resolveContentProvider(uri.getAuthority(), 0);
                    if (provider != null && getPackageName().equals(provider.packageName)) {
                        android.util.Log.i("cranpose",
                                "incoming share: skipped, own provider " + uri.getAuthority());
                        continue;
                    }
                    android.util.Log.i("cranpose", "incoming share: to native, " + uri);
                    nativeOnIncomingContent(
                            sanitize(displayName(uri, "shared")), mimeType, uri.toString());
                } catch (Exception error) {
                    android.util.Log.w("cranpose", "incoming share failed", error);
                }
            }
        }, "cranpose-incoming-share").start();
    }

    @Override
    protected void onPause() {
        cranposePaused = true;
        if (cranposeBackgroundActive) {
            startCranposeBackgroundService();
        }
        if (cranposeCamera != null) {
            cranposeCamera.stop();
        }
        super.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        cranposePaused = false;
        stopService(new Intent(this, CranposeBackgroundService.class));
    }

    @Override
    public void onRequestPermissionsResult(
            int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode == REQUEST_CAMERA && grantResults.length > 0
                && grantResults[0] == android.content.pm.PackageManager.PERMISSION_GRANTED) {
            cranposeCamera().start();
        }
    }

    @Override
    protected void onDestroy() {
        stopService(new Intent(this, CranposeBackgroundService.class));
        try {
            unregisterReceiver(cranposeUpdateInstallReceiver);
        } catch (IllegalArgumentException ignored) {
        }
        if (cranposeCamera != null) {
            cranposeCamera.stop();
        }
        if (cranposeMedia != null) {
            cranposeMedia.release();
        }
        if (cranposeNetworkCallback != null) {
            try {
                ConnectivityManager manager =
                        (ConnectivityManager) getSystemService(Context.CONNECTIVITY_SERVICE);
                if (manager != null) {
                    manager.unregisterNetworkCallback(cranposeNetworkCallback);
                }
            } catch (Exception ignored) {
            }
            cranposeNetworkCallback = null;
            nativeOnNetworkRegistration(false);
        }
        super.onDestroy();
    }

    private void dispatchDeeplink(Intent intent) {
        if (intent == null) {
            return;
        }
        String deeplink = intent.getStringExtra(EXTRA_DEEPLINK);
        if (deeplink != null && !deeplink.isEmpty()) {
            intent.removeExtra(EXTRA_DEEPLINK);
            nativeNotificationAction(deeplink);
        }
    }

    /**
     * Flattens the launching intent's extras for {@code cranpose_services::launch_args}.
     *
     * <p>A Cranpose app is a {@link NativeActivity}: it sees neither the environment of
     * the shell that ran {@code am start} nor the {@link Intent} itself, so debug and
     * instrumentation flags read from {@code std::env::var} silently return nothing on
     * device. This is the equivalent of {@code intent.extras.getBoolean(...)} in a
     * Compose activity — Rust calls it once at startup and it is pushed again from
     * {@link #onNewIntent}.
     *
     * <p>The whole {@link android.os.Bundle} crosses JNI as one string rather than one
     * call per extra. The first line is {@code 1} or {@code 0} for
     * {@code ApplicationInfo.FLAG_DEBUGGABLE}; each following line is
     * {@code <type>\t<name>\t<value>} with {@code type} one of {@code b i l f s}.
     * Extras Cranpose has no typed API for (arrays, {@code Parcelable}s, nested
     * bundles) are omitted rather than guessed at.
     */
    @SuppressWarnings("deprecation")
    public String cranposeEncodeLaunchArguments() {
        StringBuilder payload = new StringBuilder();
        payload.append(isCranposeDebuggableBuild() ? '1' : '0');
        Intent intent = getIntent();
        if (intent == null) {
            return payload.toString();
        }
        Bundle extras;
        try {
            extras = intent.getExtras();
        } catch (RuntimeException error) {
            // An extra whose class this process cannot unmarshal throws from
            // getExtras(); losing the launch arguments must not lose the launch.
            android.util.Log.w("cranpose", "launch intent extras are unreadable", error);
            return payload.toString();
        }
        if (extras == null) {
            return payload.toString();
        }
        for (String name : extras.keySet()) {
            Object value;
            try {
                value = extras.get(name);
            } catch (RuntimeException error) {
                continue;
            }
            appendLaunchArgument(payload, name, value);
        }
        return payload.toString();
    }

    /** {@code ApplicationInfo.FLAG_DEBUGGABLE} — the gate for app debug options. */
    private boolean isCranposeDebuggableBuild() {
        android.content.pm.ApplicationInfo info = getApplicationInfo();
        return info != null
                && (info.flags & android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0;
    }

    private static void appendLaunchArgument(StringBuilder payload, String name, Object value) {
        char kind;
        String encoded;
        if (value instanceof Boolean) {
            kind = 'b';
            encoded = ((Boolean) value) ? "1" : "0";
        } else if (value instanceof Integer || value instanceof Short || value instanceof Byte) {
            kind = 'i';
            encoded = Integer.toString(((Number) value).intValue());
        } else if (value instanceof Long) {
            kind = 'l';
            encoded = Long.toString((Long) value);
        } else if (value instanceof Float || value instanceof Double) {
            // `am start --ed` produces a Double; Cranpose types floats as f32.
            kind = 'f';
            encoded = Float.toString(((Number) value).floatValue());
        } else if (value instanceof CharSequence) {
            kind = 's';
            encoded = escapeLaunchArgument(value.toString());
        } else {
            return;
        }
        payload.append('\n').append(kind).append('\t')
                .append(escapeLaunchArgument(name)).append('\t').append(encoded);
    }

    /** Keeps the record separators out of names and values; {@code %} goes first. */
    private static String escapeLaunchArgument(String value) {
        return value.replace("%", "%25").replace("\t", "%09")
                .replace("\n", "%0A").replace("\r", "%0D");
    }

    private void installInsetsListener() {
        final View decor = getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener((view, insets) -> {
            pushInsets(insets);
            return view.onApplyWindowInsets(insets);
        });
        decor.post(() -> {
            WindowInsets current = decor.getRootWindowInsets();
            if (current != null) {
                pushInsets(current);
            }
        });
    }

    @SuppressWarnings("deprecation")
    private void pushInsets(WindowInsets insets) {
        int left;
        int top;
        int right;
        int bottom;
        if (Build.VERSION.SDK_INT >= 30) {
            android.graphics.Insets bars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            left = bars.left;
            top = bars.top;
            right = bars.right;
            bottom = bars.bottom;
        } else {
            left = insets.getSystemWindowInsetLeft();
            top = insets.getSystemWindowInsetTop();
            right = insets.getSystemWindowInsetRight();
            bottom = insets.getSystemWindowInsetBottom();
        }
        nativeOnInsetsChanged(left, top, right, bottom);
    }

    private boolean registerNetworkCallback() {
        ConnectivityManager manager =
                (ConnectivityManager) getSystemService(Context.CONNECTIVITY_SERVICE);
        if (manager == null) {
            nativeOnNetworkRegistration(false);
            return false;
        }
        pushNetworkStatus(manager);
        cranposeNetworkCallback = new ConnectivityManager.NetworkCallback() {
            @Override
            public void onCapabilitiesChanged(Network network, NetworkCapabilities capabilities) {
                pushNetworkStatus(manager);
            }

            @Override
            public void onLost(Network network) {
                pushNetworkStatus(manager);
            }

            @Override
            public void onAvailable(Network network) {
                pushNetworkStatus(manager);
            }
        };
        try {
            manager.registerDefaultNetworkCallback(cranposeNetworkCallback);
            nativeOnNetworkRegistration(true);
            return true;
        } catch (Exception ignored) {
            cranposeNetworkCallback = null;
            nativeOnNetworkRegistration(false);
            return false;
        }
    }

    public boolean cranposeEnsureNetworkCallback() {
        return cranposeNetworkCallback != null || registerNetworkCallback();
    }

    private void pushNetworkStatus(ConnectivityManager manager) {
        boolean online = false;
        boolean metered = true;
        try {
            Network network = manager.getActiveNetwork();
            NetworkCapabilities capabilities =
                    network == null ? null : manager.getNetworkCapabilities(network);
            online = capabilities != null
                    && capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET);
            metered = online
                    && !capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED);
        } catch (Exception ignored) {
        }
        nativeOnNetworkStatus(online, metered);
    }

    /** Performs haptic feedback ({@code kind} maps cranpose's HapticFeedback).
     * Called from Rust over JNI (any thread). */
    public void cranposeHaptic(final int kind) {
        runOnUiThread(() -> {
            View decor = getWindow().getDecorView();
            int constant;
            switch (kind) {
                case 1: constant = HapticFeedbackConstants.KEYBOARD_TAP; break;
                case 2: constant = HapticFeedbackConstants.LONG_PRESS; break;
                case 3: constant = Build.VERSION.SDK_INT >= 30
                        ? HapticFeedbackConstants.CONFIRM
                        : HapticFeedbackConstants.LONG_PRESS; break;
                case 4: constant = Build.VERSION.SDK_INT >= 30
                        ? HapticFeedbackConstants.REJECT
                        : HapticFeedbackConstants.LONG_PRESS; break;
                default: constant = Build.VERSION.SDK_INT >= 27
                        ? HapticFeedbackConstants.TEXT_HANDLE_MOVE
                        : HapticFeedbackConstants.KEYBOARD_TAP; break;
            }
            decor.performHapticFeedback(constant);
        });
    }

    /** The system vibrator, or {@code null} where the device has none. */
    private Vibrator cranposeVibrator() {
        try {
            if (Build.VERSION.SDK_INT >= 31) {
                VibratorManager manager =
                        (VibratorManager) getSystemService(Context.VIBRATOR_MANAGER_SERVICE);
                return manager == null ? null : manager.getDefaultVibrator();
            }
            return getSystemService(Vibrator.class);
        } catch (Exception ignored) {
            return null;
        }
    }

    /** Vibrates once for {@code durationMs} at {@code amplitude} (-1 for the
     * device default, otherwise 1..255). Called from Rust over JNI (any
     * thread); {@code VibrationEffect.createOneShot} needs API 26. */
    @SuppressWarnings("deprecation")
    public void cranposeHapticOneShot(final long durationMs, final int amplitude) {
        if (durationMs <= 0) {
            return;
        }
        final Vibrator vibrator = cranposeVibrator();
        if (vibrator == null || !vibrator.hasVibrator()) {
            return;
        }
        try {
            if (Build.VERSION.SDK_INT >= 26) {
                int level = amplitude < 0
                        ? VibrationEffect.DEFAULT_AMPLITUDE
                        : Math.max(1, Math.min(255, amplitude));
                vibrator.vibrate(VibrationEffect.createOneShot(durationMs, level));
            } else {
                vibrator.vibrate(durationMs);
            }
        } catch (Exception ignored) {
        }
    }

    /** Plays a vibration waveform: alternating durations in {@code timingsMs}
     * with a target amplitude each in {@code amplitudes} (0..255), looping back
     * to {@code repeat} (or -1 for a single pass). Called from Rust over JNI
     * (any thread); {@code VibrationEffect.createWaveform} needs API 26, and
     * pre-26 devices fall back to the timings alone. */
    @SuppressWarnings("deprecation")
    public void cranposeHapticWaveform(final long[] timingsMs, final int[] amplitudes,
            final int repeat) {
        if (timingsMs == null || amplitudes == null || timingsMs.length != amplitudes.length
                || timingsMs.length == 0) {
            return;
        }
        final Vibrator vibrator = cranposeVibrator();
        if (vibrator == null || !vibrator.hasVibrator()) {
            return;
        }
        final int index = repeat >= 0 && repeat < timingsMs.length ? repeat : -1;
        try {
            if (Build.VERSION.SDK_INT >= 26) {
                int[] levels = new int[amplitudes.length];
                for (int i = 0; i < amplitudes.length; i++) {
                    levels[i] = Math.max(0, Math.min(255, amplitudes[i]));
                }
                vibrator.vibrate(VibrationEffect.createWaveform(timingsMs, levels, index));
            } else {
                vibrator.vibrate(timingsMs, index);
            }
        } catch (Exception ignored) {
        }
    }

    /** Plays a predefined effect: 0 click, 1 double click, 2 tick, 3 heavy
     * click. Called from Rust over JNI (any thread);
     * {@code VibrationEffect.createPredefined} needs API 29, and older devices
     * fall back to a short one-shot of comparable weight. */
    public void cranposeHapticPredefined(final int effect) {
        final Vibrator vibrator = cranposeVibrator();
        if (vibrator == null || !vibrator.hasVibrator()) {
            return;
        }
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                int constant;
                switch (effect) {
                    case 1: constant = VibrationEffect.EFFECT_DOUBLE_CLICK; break;
                    case 2: constant = VibrationEffect.EFFECT_TICK; break;
                    case 3: constant = VibrationEffect.EFFECT_HEAVY_CLICK; break;
                    default: constant = VibrationEffect.EFFECT_CLICK; break;
                }
                vibrator.vibrate(VibrationEffect.createPredefined(constant));
            } else {
                long duration;
                switch (effect) {
                    case 1: duration = 40L; break;
                    case 2: duration = 8L; break;
                    case 3: duration = 50L; break;
                    default: duration = 20L; break;
                }
                cranposeHapticOneShot(duration, -1);
            }
        } catch (Exception ignored) {
        }
    }

    /** Stops any vibration in progress, including a repeating waveform.
     * Called from Rust over JNI (any thread). */
    public void cranposeHapticCancel() {
        final Vibrator vibrator = cranposeVibrator();
        if (vibrator == null) {
            return;
        }
        try {
            vibrator.cancel();
        } catch (Exception ignored) {
        }
    }

    /** Whether the vibrator reproduces amplitudes rather than treating every
     * non-zero level as full strength. Called from Rust over JNI (any thread);
     * blocks only for the duration of the query. */
    public boolean cranposeHapticHasAmplitudeControl() {
        final Vibrator vibrator = cranposeVibrator();
        if (vibrator == null || !vibrator.hasVibrator()) {
            return false;
        }
        try {
            return Build.VERSION.SDK_INT >= 26 && vibrator.hasAmplitudeControl();
        } catch (Exception ignored) {
            return false;
        }
    }

    /** Writes {@code text} to the system clipboard. Called from Rust over JNI. */
    public void cranposeClipboardSet(final String text) {
        runOnUiThread(() -> {
            ClipboardManager clipboard =
                    (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
            if (clipboard != null) {
                clipboard.setPrimaryClip(ClipData.newPlainText("cranpose", text));
            }
        });
    }

    /** Reads the system clipboard's text (empty when unavailable). Blocks the
     * calling (native) thread briefly for the main-thread hop. */
    public String cranposeClipboardGet() {
        final String[] result = {""};
        final java.util.concurrent.CountDownLatch latch =
                new java.util.concurrent.CountDownLatch(1);
        runOnUiThread(() -> {
            try {
                ClipboardManager clipboard =
                        (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
                if (clipboard != null && clipboard.hasPrimaryClip()) {
                    ClipData clip = clipboard.getPrimaryClip();
                    if (clip != null && clip.getItemCount() > 0) {
                        CharSequence text = clip.getItemAt(0).coerceToText(CranposeActivity.this);
                        if (text != null) {
                            result[0] = text.toString();
                        }
                    }
                }
            } finally {
                latch.countDown();
            }
        });
        try {
            latch.await(250, java.util.concurrent.TimeUnit.MILLISECONDS);
        } catch (InterruptedException ignored) {
            Thread.currentThread().interrupt();
        }
        return result[0];
    }

    /** Stages {@code bytes} and presents the system share sheet. Called from
     * Rust over JNI. */
    public void cranposeShare(final String fileName, final String mimeType,
            final byte[] bytes, final String text) {
        new Thread(() -> {
            try {
                java.io.File dir = new java.io.File(getCacheDir(), "cranpose_share");
                //noinspection ResultOfMethodCallIgnored
                dir.mkdirs();
                java.io.File staged = new java.io.File(dir, sanitizeFileName(fileName));
                try (OutputStream out = new java.io.FileOutputStream(staged)) {
                    out.write(bytes);
                }
                final Uri uri = CranposeShareProvider.uriFor(this, staged);
                runOnUiThread(() -> {
                    Intent send = new Intent(Intent.ACTION_SEND);
                    send.setType(mimeType);
                    send.putExtra(Intent.EXTRA_STREAM, uri);
                    if (text != null && !text.isEmpty()) {
                        send.putExtra(Intent.EXTRA_TEXT, text);
                        send.putExtra(Intent.EXTRA_SUBJECT, text);
                    }
                    send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
                    startActivity(Intent.createChooser(send, null));
                });
            } catch (Exception error) {
                android.util.Log.w("cranpose", "share failed", error);
            }
        }).start();
    }

    @SuppressWarnings("deprecation")
    private static Uri incomingStream(Intent intent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            return intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri.class);
        }
        return intent.getParcelableExtra(Intent.EXTRA_STREAM);
    }

    @SuppressWarnings("deprecation")
    private static ArrayList<Uri> incomingStreams(Intent intent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            return intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri.class);
        }
        return intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
    }

    private String sanitizeFileName(String name) {
        String cleaned = name == null ? "file" : name.replaceAll("[/\\\\ ]", "_");
        return cleaned.isEmpty() ? "file" : cleaned;
    }

    /** Asks for POST_NOTIFICATIONS on Android 13+. Called from Rust over JNI. */
    public void cranposeNotifyRequestPermission() {
        if (Build.VERSION.SDK_INT >= 33
                && checkSelfPermission("android.permission.POST_NOTIFICATIONS")
                        != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            runOnUiThread(() -> requestPermissions(
                    new String[] {"android.permission.POST_NOTIFICATIONS"},
                    REQ_NOTIFY_PERMISSION));
        }
    }

    /** Posts (or replaces, by {@code tag}) a notification. Called from Rust
     * over JNI. */
    @SuppressWarnings("deprecation")
    public void cranposeNotify(final String tag, final String title, final String body,
            final boolean ongoing, final String deeplink) {
        runOnUiThread(() -> {
            NotificationManager manager =
                    (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
            if (manager == null) {
                return;
            }
            if (Build.VERSION.SDK_INT >= 26
                    && manager.getNotificationChannel(NOTIFY_CHANNEL) == null) {
                manager.createNotificationChannel(new NotificationChannel(
                        NOTIFY_CHANNEL, "App", NotificationManager.IMPORTANCE_DEFAULT));
            }
            Notification.Builder builder = Build.VERSION.SDK_INT >= 26
                    ? new Notification.Builder(this, NOTIFY_CHANNEL)
                    : new Notification.Builder(this);
            builder.setContentTitle(title)
                    .setContentText(body)
                    .setSmallIcon(getApplicationInfo().icon)
                    .setOngoing(ongoing)
                    .setAutoCancel(!ongoing);
            if (deeplink != null && !deeplink.isEmpty()) {
                Intent launch = getPackageManager()
                        .getLaunchIntentForPackage(getPackageName());
                if (launch != null) {
                    launch.putExtra(EXTRA_DEEPLINK, deeplink);
                    launch.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP
                            | Intent.FLAG_ACTIVITY_CLEAR_TOP);
                    builder.setContentIntent(PendingIntent.getActivity(
                            this,
                            tag.hashCode(),
                            launch,
                            PendingIntent.FLAG_UPDATE_CURRENT
                                    | PendingIntent.FLAG_IMMUTABLE));
                }
            }
            manager.notify(tag, 1, builder.build());
        });
    }

    /** Cancels a notification posted through {@link #cranposeNotify}. */
    public void cranposeNotifyCancel(final String tag) {
        runOnUiThread(() -> {
            NotificationManager manager =
                    (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
            if (manager != null) {
                manager.cancel(tag, 1);
            }
        });
    }

    /** Presents ACTION_CREATE_DOCUMENT so the user names a destination. The new
     * document's URI comes back through {@link #nativeOnDocumentCreated}; the
     * caller then streams into it with {@link #cranposeOpenUriWrite}. Called
     * from Rust over JNI. */
    public void cranposeCreateDocument(final long token, final String fileName,
            final String mimeType) {
        runOnUiThread(() -> {
            pendingToken = token;
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(mimeType == null || mimeType.isEmpty()
                    ? "application/octet-stream"
                    : mimeType);
            intent.putExtra(Intent.EXTRA_TITLE, fileName);
            launch(intent, token, FLAG_SAVE);
        });
    }
}
