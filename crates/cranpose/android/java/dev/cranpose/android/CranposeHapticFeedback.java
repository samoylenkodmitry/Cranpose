package dev.cranpose.android;

import android.os.Build;
import android.view.HapticFeedbackConstants;

/**
 * The {@link HapticFeedbackConstants} value for a kind of Cranpose's
 * {@code HapticFeedback}, which Rust sends as a number: 0 a light impact or a
 * selection change, 1 a medium impact, 2 a heavy impact, 3 a success, 4 a
 * warning or an error.
 */
final class CranposeHapticFeedback {
    private CranposeHapticFeedback() {}

    static int constant(int kind) {
        switch (kind) {
            case 1:
                return HapticFeedbackConstants.KEYBOARD_TAP;
            case 2:
                return HapticFeedbackConstants.LONG_PRESS;
            case 3:
                return Build.VERSION.SDK_INT >= 30
                        ? HapticFeedbackConstants.CONFIRM
                        : HapticFeedbackConstants.LONG_PRESS;
            case 4:
                return Build.VERSION.SDK_INT >= 30
                        ? HapticFeedbackConstants.REJECT
                        : HapticFeedbackConstants.LONG_PRESS;
            default:
                return Build.VERSION.SDK_INT >= 27
                        ? HapticFeedbackConstants.TEXT_HANDLE_MOVE
                        : HapticFeedbackConstants.KEYBOARD_TAP;
        }
    }
}
