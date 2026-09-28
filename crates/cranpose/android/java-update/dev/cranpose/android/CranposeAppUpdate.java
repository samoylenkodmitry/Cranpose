package dev.cranpose.android;

import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageInstaller;
import android.os.Build;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Locale;

import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Checks a GitHub release for a newer package and installs it through
 * Android's {@link PackageInstaller}, behind {@code cranpose_services::app_update}.
 *
 * <p>An application carries this class only when its build script declares
 * {@code Use::update()}. From that declaration the Cranpose Gradle plugin adds
 * this source directory, and declares this class in the manifest as a receiver
 * with {@code android:exported="false"}. The installer reports each session's
 * status to that receiver through an explicit {@link PendingIntent}, so no
 * other application can deliver an intent to it on any Android version.
 *
 * <p>The receiver acts only on the session this process committed. Anything
 * else, such as a status that arrives after the process restarted, is ignored.
 */
public final class CranposeAppUpdate extends BroadcastReceiver {
    private static native void nativeOnAppUpdateStatus(int kind, String version,
            String downloadUrl, long downloaded, long total, String message, String digest);

    private static final int UPDATE_CHECKING = 1;
    private static final int UPDATE_CURRENT = 2;
    private static final int UPDATE_AVAILABLE = 3;
    private static final int UPDATE_DOWNLOADING = 4;
    private static final int UPDATE_CONFIRMATION = 5;
    private static final int UPDATE_INSTALLING = 6;
    private static final int UPDATE_ERROR = 7;
    private static final int UPDATE_VERIFYING = 8;

    private static final int NO_SESSION = -1;

    private static volatile int committedSession = NO_SESSION;

    /**
     * Queries a GitHub repository's latest release and selects one package
     * asset. Called from Rust over JNI.
     */
    public static void cranposeCheckGitHubUpdate(Context context,
            String repository, String currentVersion, String assetSuffix) {
        final String agent = context.getPackageName();
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
                connection.setRequestProperty("User-Agent", agent);
                connection.setRequestProperty("Accept", "application/vnd.github+json");
                connection.setConnectTimeout(15000);
                connection.setReadTimeout(15000);
                int code = connection.getResponseCode();
                if (code != HttpURLConnection.HTTP_OK) {
                    throw new IOException("GitHub returned HTTP " + code);
                }
                JSONObject release = new JSONObject(readText(connection.getInputStream()));
                String tag = release.optString("tag_name", "");
                String latest = tag.startsWith("v") ? tag.substring(1) : tag;
                String packageUrl = "";
                long packageSize = 0;
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
                if (isNewerVersion(latest, current)) {
                    nativeOnAppUpdateStatus(
                            UPDATE_AVAILABLE, latest, packageUrl, 0, packageSize, "",
                            packageDigest);
                } else {
                    nativeOnAppUpdateStatus(UPDATE_CURRENT, "", "", 0, 0, "", "");
                }
            } catch (Exception error) {
                reportError(error);
            }
        }, "cranpose-update-check").start();
    }

    /**
     * Downloads a package, checks it against the digest the release feed
     * published, and hands it to Android's platform installer. Called from
     * Rust over JNI.
     *
     * <p>The digest is checked <em>before</em> the session is committed, and the
     * session is abandoned when it does not match, so a package that arrived
     * corrupted or was swapped in transit never reaches the installer. Android's
     * own signature check still applies afterwards and catches a package signed
     * by someone else; it does not catch one that arrived damaged.
     *
     * @param context      the activity the installer's confirmation returns to
     * @param downloadUrl  where the package is fetched from
     * @param digestSpec   {@code sha256:<hex>}; the framework refuses a package
     *                     without one before this is called
     * @param expectedSize the size the feed published, or {@code 0}
     */
    public static void cranposeInstallUpdate(Context context,
            String downloadUrl, String digestSpec, long expectedSize) {
        final Context app = context.getApplicationContext();
        final String source = downloadUrl == null ? "" : downloadUrl.trim();
        final String digestRequest = digestSpec == null ? "" : digestSpec.trim();
        final long announcedSize = Math.max(expectedSize, 0);
        new Thread(() -> {
            PackageInstaller.Session session = null;
            try {
                MessageDigest digest = digestFor(digestRequest);
                HttpURLConnection connection = (HttpURLConnection) new URL(source).openConnection();
                connection.setRequestProperty("User-Agent", app.getPackageName());
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

                PackageInstaller installer = app.getPackageManager().getPackageInstaller();
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
                String actual = hex(digest.digest());
                String expected = digestValue(digestRequest);
                if (!actual.equals(expected)) {
                    throw new IOException(
                            "the downloaded package does not match its digest (expected "
                                    + expected + ", got " + actual + ")");
                }
                if (announcedSize > 0 && written != announcedSize) {
                    throw new IOException("the downloaded package is " + written
                            + " bytes, and the release feed said " + announcedSize);
                }

                Intent result = new Intent(app, CranposeAppUpdate.class);
                int flags = PendingIntent.FLAG_UPDATE_CURRENT;
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    flags |= PendingIntent.FLAG_MUTABLE;
                }
                PendingIntent pending = PendingIntent.getBroadcast(app, sessionId, result, flags);
                committedSession = sessionId;
                session.commit(pending.getIntentSender());
                session.close();
                session = null;
                nativeOnAppUpdateStatus(UPDATE_INSTALLING, "", "", 0, total, "", "");
            } catch (Exception error) {
                if (session != null) {
                    session.abandon();
                    session.close();
                }
                reportError(error);
            }
        }, "cranpose-update-install").start();
    }

    /** Receives the installer's status for the session this process committed. */
    @Override
    public void onReceive(Context context, Intent intent) {
        int session = intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, NO_SESSION);
        if (session == NO_SESSION || session != committedSession) {
            return;
        }
        int status = intent.getIntExtra(
                PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
        if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
            nativeOnAppUpdateStatus(UPDATE_CONFIRMATION, "", "", 0, 0, "", "");
            Intent confirmation = confirmationOf(intent);
            if (confirmation != null) {
                confirmation.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                try {
                    context.startActivity(confirmation);
                } catch (Exception error) {
                    reportError(error);
                }
            }
            return;
        }
        committedSession = NO_SESSION;
        if (status == PackageInstaller.STATUS_SUCCESS) {
            nativeOnAppUpdateStatus(UPDATE_INSTALLING, "", "", 0, 0, "", "");
        } else {
            String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
            nativeOnAppUpdateStatus(UPDATE_ERROR, "", "", 0, 0,
                    message == null ? "installation failed" : message, "");
        }
    }

    @SuppressWarnings("deprecation")
    private static Intent confirmationOf(Intent intent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            return intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class);
        }
        return intent.getParcelableExtra(Intent.EXTRA_INTENT);
    }

    private static MessageDigest digestFor(String digestSpec) throws IOException {
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
        if (digestValue(digestSpec).isEmpty()) {
            throw new IOException("empty package digest: " + digestSpec);
        }
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (NoSuchAlgorithmException error) {
            throw new IOException("this device cannot compute SHA-256", error);
        }
    }

    private static String digestValue(String digestSpec) {
        int separator = digestSpec.indexOf(':');
        if (separator < 0) {
            return "";
        }
        return digestSpec.substring(separator + 1).trim().toLowerCase(Locale.ROOT);
    }

    private static String hex(byte[] bytes) {
        StringBuilder out = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            out.append(Character.forDigit((value >> 4) & 0x0f, 16));
            out.append(Character.forDigit(value & 0x0f, 16));
        }
        return out.toString();
    }

    private static String readText(InputStream input) throws IOException {
        try (InputStream stream = input; ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[8192];
            int read;
            while ((read = stream.read(buffer)) >= 0) {
                output.write(buffer, 0, read);
            }
            return output.toString("UTF-8");
        }
    }

    private static boolean isNewerVersion(String latest, String current) {
        int[] remote = parseVersion(latest);
        int[] running = parseVersion(current);
        for (int index = 0; index < remote.length; index++) {
            if (remote[index] != running[index]) {
                return remote[index] > running[index];
            }
        }
        return false;
    }

    private static int[] parseVersion(String version) {
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

    private static void reportError(Throwable error) {
        String message = error.getMessage();
        nativeOnAppUpdateStatus(UPDATE_ERROR, "", "", 0, 0,
                message == null ? error.getClass().getSimpleName() : message.replace('\n', ' '),
                "");
    }
}
