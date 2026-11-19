package dev.cranpose.android;

import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageManager;
import java.util.ArrayList;
import java.util.List;

/**
 * Runtime permissions that differ between devices, behind
 * {@code cranpose::android_permission} and
 * {@code cranpose::request_android_permissions}.
 *
 * <p>One APK names the permissions of every headset: a Meta Quest defines the
 * {@code com.oculus.permission} names, Android XR the {@code android.permission}
 * ones, and a device ignores the names it does not define. An application
 * carries this class only when its build script declares a service that asks
 * for such permissions, such as {@code Use::hand_tracking()}.
 *
 * <p>Nothing here asks the person on its own. The prompt shows only from
 * {@link #cranposeRequestPermissions}. The answer reaches
 * {@code CranposeActivity#onRequestPermissionsResult}, which keeps a refusal
 * and hands the result to Rust.
 */
public final class CranposePermissions {
    private static final int REQUEST_CODE = 0x0C9A0000 + 0x400;

    private CranposePermissions() {}

    /**
     * The answer for a group of alternative permissions, newline separated: 0
     * not asked, 1 granted, 2 refused, 3 refused for good, so that Android
     * shows no prompt any more and only the application's settings can allow
     * it. Any one alternative that is granted grants the group. A group of
     * which this device defines no permission needs none, which is how a
     * headset that gates its trackers by no permission gets them at once, and
     * it counts as granted.
     */
    public static int cranposePermissionState(Activity activity, String alternatives) {
        PackageManager packages = activity.getPackageManager();
        boolean defined = false;
        boolean askAgain = false;
        boolean refusedBefore = false;
        for (String permission : alternatives.split("\n")) {
            if (activity.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED) {
                return 1;
            }
            if (!isDefined(packages, permission)) {
                continue;
            }
            defined = true;
            // After one refusal Android wants the reason shown before it asks
            // again; once a refused permission no longer wants it, its prompt
            // does not show. A prompt that is open or was closed unanswered
            // leaves the permission not asked.
            askAgain |= activity.shouldShowRequestPermissionRationale(permission);
            refusedBefore |= activity.getSharedPreferences(
                            CranposeActivity.REFUSED_PERMISSIONS, Context.MODE_PRIVATE)
                    .getBoolean(permission, false);
        }
        if (!defined) {
            return 1;
        }
        if (askAgain) {
            return 2;
        }
        return refusedBefore ? 3 : 0;
    }

    /**
     * Shows the system prompt for those of the permissions, newline separated,
     * that the application does not hold. A permission that the device does
     * not define is refused without a prompt. Called only on the application's
     * explicit request.
     */
    public static void cranposeRequestPermissions(Activity activity, String permissions) {
        List<String> wanted = new ArrayList<>();
        for (String permission : permissions.split("\n")) {
            if (activity.checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
                wanted.add(permission);
            }
        }
        if (wanted.isEmpty()) {
            return;
        }
        String[] names = wanted.toArray(new String[0]);
        activity.runOnUiThread(() -> activity.requestPermissions(names, REQUEST_CODE));
    }

    // Whether this device defines the permission
    private static boolean isDefined(PackageManager packages, String permission) {
        try {
            packages.getPermissionInfo(permission, 0);
            return true;
        } catch (PackageManager.NameNotFoundException notDefined) {
            return false;
        }
    }
}
