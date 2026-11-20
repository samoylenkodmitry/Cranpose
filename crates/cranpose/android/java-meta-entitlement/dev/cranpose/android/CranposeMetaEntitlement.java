package dev.cranpose.android;

import android.app.Activity;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.util.Log;

import com.meta.horizon.platform.ovr.Core;
import com.meta.horizon.platform.ovr.Error;
import com.meta.horizon.platform.ovr.enums.PlatformInitializeResult;
import com.meta.horizon.platform.ovr.models.PlatformInitialize;
import com.meta.horizon.platform.ovr.requests.Entitlements;
import com.meta.horizon.platform.ovr.requests.Request;

/**
 * The entitlement check of the Meta Horizon Store, behind {@code cranpose::meta_entitlement}.
 *
 * <p>This class lives in its own source directory, {@code
 * crates/cranpose/android/java-meta-entitlement}, because it is the only part of Cranpose that
 * needs the Meta Platform SDK ({@code com.meta.horizon.platform.ovr}). An app that does not
 * list the {@code meta-entitlement} service in its Gradle build carries neither this class nor
 * the SDK. The SDK is under the Meta Platform Technologies SDK License Agreement, which binds
 * whoever downloads or uses it.
 *
 * <p>The check follows Meta's guide for the Android Platform SDK: initialize the SDK with the
 * app's id, then ask whether the viewer is entitled to the app. The app id comes from the
 * {@code com.meta.horizon.platform.ovr.OCULUS_APP_ID} entry of the merged manifest, which the
 * Gradle plugin fills from the build's {@code cranposeMetaAppId}. Any exception counts as a
 * failed check, as the guide says.
 *
 * <p>Threading. {@link #cranposeMetaEntitlementBegin} runs on the native-activity thread and
 * returns at once. The SDK answers on the Java main thread. The answer sits in a volatile
 * field that the Rust side polls through {@link #cranposeMetaEntitlementState}.
 */
public final class CranposeMetaEntitlement {

    private static final String TAG = "cranpose";

    /** The key of the app id, which the Meta SDK also reads when no id is passed. */
    private static final String APP_ID_KEY = "com.meta.horizon.platform.ovr.OCULUS_APP_ID";

    /** Mirrors {@code MetaEntitlementState} in {@code cranpose::android::meta_entitlement}. */
    private static final int STATE_NOT_STARTED = 0;
    private static final int STATE_CHECKING = 1;
    private static final int STATE_ENTITLED = 2;
    private static final int STATE_NOT_ENTITLED = 3;

    private static volatile int state = STATE_NOT_STARTED;
    private static volatile String detail = "";

    private CranposeMetaEntitlement() {}

    /**
     * Starts the check. A check that runs and a check that found the viewer entitled keep
     * their state, so a process that starts its immersive loop again does not ask the platform
     * again. A check that failed starts anew. The process of a quit app stays cached. A failure
     * that came from a busy platform must not lock the owner out of it.
     */
    public static synchronized void cranposeMetaEntitlementBegin(Activity activity) {
        if (state == STATE_CHECKING || state == STATE_ENTITLED) {
            return;
        }
        detail = "";
        state = STATE_CHECKING;
        try {
            String appId = appId(activity);
            Log.i(TAG, "Meta entitlement: the check starts for app " + appId);
            Request<PlatformInitialize> initializing = Core.asyncInitialize(appId, activity);
            if (initializing == null) {
                // The SDK was initialized before this call
                askViewer();
                return;
            }
            initializing
                .onSuccess(
                    new Request.Handler<PlatformInitialize>() {
                        @Override
                        public void handle(PlatformInitialize initialized) {
                            PlatformInitializeResult result = initialized.getResult();
                            if (result == PlatformInitializeResult.Success
                                    || result == PlatformInitializeResult.PreLoaded) {
                                askViewer();
                            } else {
                                finish(STATE_NOT_ENTITLED, "the SDK did not initialize: " + result);
                            }
                        }
                    })
                .onError(
                    new Request.Handler<Error>() {
                        @Override
                        public void handle(Error error) {
                            finish(STATE_NOT_ENTITLED, "the SDK did not initialize: " + describe(error));
                        }
                    });
        } catch (Throwable error) {
            finish(STATE_NOT_ENTITLED, "the check threw " + error);
        }
    }

    /**
     * Where the check stands: 0 not started, 1 running, 2 the viewer is entitled and 3 the viewer
     * is not or the check failed.
     */
    public static int cranposeMetaEntitlementState() {
        return state;
    }

    /** Why the check ended as it did, for the log. Empty while it runs. */
    public static String cranposeMetaEntitlementDetail() {
        return detail;
    }

    private static void askViewer() {
        try {
            Entitlements.getIsViewerEntitled()
                .onSuccess(
                    new Request.Handler<Object>() {
                        @Override
                        public void handle(Object ignored) {
                            finish(STATE_ENTITLED, "the viewer is entitled");
                        }
                    })
                .onError(
                    new Request.Handler<Error>() {
                        @Override
                        public void handle(Error error) {
                            finish(STATE_NOT_ENTITLED, "the viewer is not entitled: " + describe(error));
                        }
                    });
        } catch (Throwable error) {
            finish(STATE_NOT_ENTITLED, "the check threw " + error);
        }
    }

    private static synchronized void finish(int result, String why) {
        detail = why;
        state = result;
        Log.i(TAG, "Meta entitlement: " + why);
    }

    private static String describe(Error error) {
        return error == null
            ? "no error"
            : "code " + error.getCode() + ", http " + error.getHttpCode() + ", " + error.getMessage();
    }

    /**
     * The app id from the merged manifest. The manifest compiler stores an all-digit value as a
     * number, so the entry is read as an object and turned into text.
     */
    private static String appId(Activity activity) throws PackageManager.NameNotFoundException {
        ApplicationInfo info =
            activity.getPackageManager()
                .getApplicationInfo(activity.getPackageName(), PackageManager.GET_META_DATA);
        Bundle data = info.metaData;
        Object value = data == null ? null : data.get(APP_ID_KEY);
        if (value == null) {
            throw new IllegalStateException(
                "the manifest has no " + APP_ID_KEY + "; set cranposeMetaAppId in the Gradle build");
        }
        return String.valueOf(value);
    }
}
