package dev.cranpose.android;

import android.app.Activity;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.provider.Settings;

/**
 * Permission to record, behind {@code cranpose_services::microphone_permission}.
 *
 * <p>An application carries this class only when its build script declares
 * {@code Use::microphone(...)}; from that declaration the Cranpose Gradle
 * plugin adds this source directory and the RECORD_AUDIO permission.
 *
 * <p>Nothing here asks the person on its own. The prompt shows only from
 * {@link #cranposeMicrophoneRequestPermission}, which Rust calls only when the
 * application calls {@code request_microphone_permission()}. The answer
 * reaches {@code CranposeActivity#onRequestPermissionsResult}, which hands any
 * result it did not ask for itself to Rust.
 */
public final class CranposeMicrophone {
    private static final String PERMISSION = "android.permission.RECORD_AUDIO";
    private static final String PREFERENCES = "dev.cranpose.microphone";
    private static final String ASKED = "asked";
    private static final int REQUEST_CODE = 0x0C9A0000 + 0x300;

    private CranposeMicrophone() {}

    /**
     * 0 not asked, 1 granted, 2 refused, 3 refused for good: Android shows no
     * prompt any more and only the application's settings can allow it.
     */
    public static int cranposeMicrophonePermission(Activity activity) {
        if (activity.checkSelfPermission(PERMISSION) == PackageManager.PERMISSION_GRANTED) {
            return 1;
        }
        boolean asked = activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .getBoolean(ASKED, false);
        if (!asked) {
            return 0;
        }
        // After one refusal Android wants the reason shown before it asks
        // again; once it no longer does, its prompt does not show.
        return activity.shouldShowRequestPermissionRationale(PERMISSION) ? 2 : 3;
    }

    /** Shows the system prompt. Called only on the application's explicit request. */
    public static void cranposeMicrophoneRequestPermission(Activity activity) {
        activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putBoolean(ASKED, true)
                .apply();
        activity.runOnUiThread(() ->
                activity.requestPermissions(new String[] {PERMISSION}, REQUEST_CODE));
    }

    /** Opens this application's page in the system settings. */
    public static void cranposeMicrophoneOpenSettings(Activity activity) {
        Intent settings = new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                Uri.fromParts("package", activity.getPackageName(), null));
        activity.runOnUiThread(() -> activity.startActivity(settings));
    }
}
