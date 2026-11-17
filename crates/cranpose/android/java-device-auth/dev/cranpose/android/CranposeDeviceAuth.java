package dev.cranpose.android;

import android.annotation.TargetApi;
import android.app.Activity;
import android.app.KeyguardManager;
import android.content.Intent;
import android.hardware.biometrics.BiometricManager;
import android.hardware.biometrics.BiometricPrompt;
import android.os.Build;
import android.os.CancellationSignal;
import android.util.Log;

/**
 * Asking the owner of the device to unlock, behind
 * {@code cranpose_services::device_auth}.
 *
 * <p>An application carries this class only when its build script declares
 * {@code Use::device_auth(...)}; from that declaration the Cranpose Gradle
 * plugin adds this source directory and the USE_BIOMETRIC permission.
 *
 * <p>Android 10 and newer show the system's {@link BiometricPrompt} with the
 * screen lock allowed, so a person without a face or a fingerprint enrolled,
 * or whose finger fails, unlocks with the PIN, pattern or password. Android 9
 * and older show the confirm-credential screen of {@link KeyguardManager},
 * whose answer reaches {@code CranposeActivity#onActivityResult}, which hands
 * every result it did not ask for itself to Rust.
 *
 * <p>Nothing here prompts on its own: Rust calls
 * {@link #cranposeDeviceAuthAuthenticate} only when the application calls
 * {@code authenticate_device_owner}.
 */
public final class CranposeDeviceAuth {
    private static final String TAG = "CranposeDeviceAuth";

    /** The same number as {@code device_auth_wire::CONFIRM_CREDENTIAL_REQUEST}. */
    static final int CONFIRM_CREDENTIAL_REQUEST = 0x0C9B0001;

    static final int KIND_SUCCEEDED = 0;
    static final int KIND_ERROR = 1;
    static final int KIND_NOT_SHOWN = 2;
    static final int KIND_NO_SCREEN_LOCK = 3;

    private static native void nativeOnDeviceAuthResult(long request, int kind, int code);

    private CranposeDeviceAuth() {}

    /** 0 no screen lock, 1 the screen lock only, 2 a face or a fingerprint as well. */
    public static int cranposeDeviceAuthAvailability(Activity activity) {
        KeyguardManager keyguard = activity.getSystemService(KeyguardManager.class);
        if (keyguard == null || !keyguard.isDeviceSecure()) {
            return 0;
        }
        if (Build.VERSION.SDK_INT >= 29) {
            return biometricEnrolled(activity) ? 2 : 1;
        }
        // Android 9 and older ask with the screen lock alone.
        return 1;
    }

    /** Shows the prompt; the outcome reaches Rust through {@code nativeOnDeviceAuthResult}. */
    public static void cranposeDeviceAuthAuthenticate(
            Activity activity, long request, String reason) {
        activity.runOnUiThread(() -> {
            try {
                KeyguardManager keyguard = activity.getSystemService(KeyguardManager.class);
                if (keyguard == null || !keyguard.isDeviceSecure()) {
                    nativeOnDeviceAuthResult(request, KIND_NO_SCREEN_LOCK, 0);
                } else if (Build.VERSION.SDK_INT >= 29) {
                    showPrompt(activity, request, reason);
                } else {
                    confirmCredential(activity, keyguard, request, reason);
                }
            } catch (RuntimeException error) {
                Log.w(TAG, "the unlock prompt did not open", error);
                nativeOnDeviceAuthResult(request, KIND_NOT_SHOWN, 0);
            }
        });
    }

    @TargetApi(29)
    @SuppressWarnings("deprecation")
    private static boolean biometricEnrolled(Activity activity) {
        BiometricManager biometrics = activity.getSystemService(BiometricManager.class);
        if (biometrics == null) {
            return false;
        }
        int answer = Build.VERSION.SDK_INT >= 30
                ? biometrics.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_WEAK)
                : biometrics.canAuthenticate();
        return answer == BiometricManager.BIOMETRIC_SUCCESS;
    }

    @TargetApi(29)
    @SuppressWarnings("deprecation")
    private static void showPrompt(Activity activity, long request, String reason) {
        BiometricPrompt.Builder builder = new BiometricPrompt.Builder(activity).setTitle(reason);
        if (Build.VERSION.SDK_INT >= 30) {
            builder.setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_WEAK
                    | BiometricManager.Authenticators.DEVICE_CREDENTIAL);
        } else {
            builder.setDeviceCredentialAllowed(true);
        }
        builder.build().authenticate(new CancellationSignal(), activity.getMainExecutor(),
                new BiometricPrompt.AuthenticationCallback() {
                    @Override
                    public void onAuthenticationSucceeded(
                            BiometricPrompt.AuthenticationResult result) {
                        nativeOnDeviceAuthResult(request, KIND_SUCCEEDED, 0);
                    }

                    @Override
                    public void onAuthenticationError(int code, CharSequence message) {
                        nativeOnDeviceAuthResult(request, KIND_ERROR, code);
                    }

                    // onAuthenticationFailed is a face or finger that did not
                    // match. The prompt stays open for the next try, so the
                    // request is not over.
                });
    }

    @SuppressWarnings("deprecation")
    private static void confirmCredential(
            Activity activity, KeyguardManager keyguard, long request, String reason) {
        Intent intent = keyguard.createConfirmDeviceCredentialIntent(reason, null);
        if (intent == null) {
            nativeOnDeviceAuthResult(request, KIND_NO_SCREEN_LOCK, 0);
            return;
        }
        activity.startActivityForResult(intent, CONFIRM_CREDENTIAL_REQUEST);
    }
}
