package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.accessibilityservice.AccessibilityServiceInfo;
import android.content.Intent;
import android.graphics.Rect;
import android.os.SystemClock;
import android.view.KeyEvent;
import android.view.MotionEvent;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import dev.cranpose.android.CranposeActivity;
import java.io.File;
import org.junit.Test;
import org.junit.runner.RunWith;
import static com.compose_rs.demo.ImeTestSupport.*;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

@RunWith(AndroidJUnit4.class)
public final class CranposeInsetsTest {
    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();

    @Test
    public void keyboardMovesLayoutPaddingAndUpdatesInsetReaders() throws Exception {
        UiAutomation automation = instrumentation.getUiAutomation();
        AccessibilityServiceInfo info = automation.getServiceInfo();
        info.flags |= AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        automation.setServiceInfo(info);
        Intent intent = new Intent(instrumentation.getTargetContext(), CranposeActivity.class)
                .putExtra("test_screen", "insets_robot");
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(intent)) {
            scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(true));
            awaitNode(automation, "Insets closed");
            Rect before = bounds(awaitNode(automation, "Insets bottom marker"));
            Rect field = bounds(awaitNode(automation, "Insets editor"));
            long time = SystemClock.uptimeMillis();
            for (int action : new int[] {MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP}) {
                MotionEvent event = MotionEvent.obtain(time, SystemClock.uptimeMillis(), action, field.centerX(), field.centerY(), 0);
                instrumentation.sendPointerSync(event);
                event.recycle();
            }
            awaitNode(automation, "Insets open");
            Rect keyboard = awaitKeyboard(automation, true);
            Rect after = awaitMarker(automation, before, true);
            assertTrue("Keyboard must lift the bottom marker", after.bottom < before.bottom - 100);
            Rect editor = bounds(awaitNode(automation, "Insets editor"));
            assertTrue("Focused editor stays above the bottom marker", editor.bottom <= after.bottom);
            assertTrue("Marker stays above keyboard", after.bottom <= keyboard.top);
            screenshot(automation, new File(instrumentation.getTargetContext().getExternalFilesDir(null), "insets-open.png"));
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            awaitNode(automation, "Insets closed");
            awaitKeyboard(automation, false);
            Rect restored = awaitMarker(automation, before, false);
            assertEquals("Hiding the keyboard restores padding", before.bottom, restored.bottom);
            screenshot(automation, new File(instrumentation.getTargetContext().getExternalFilesDir(null), "insets-closed.png"));
        }
    }

    private static Rect awaitMarker(UiAutomation automation, Rect before, boolean open) {
        long until = SystemClock.uptimeMillis() + 5000;
        Rect result;
        do {
            result = bounds(awaitNode(automation, "Insets bottom marker"));
            if (open ? result.bottom < before.bottom - 100 : result.bottom == before.bottom) return result;
            SystemClock.sleep(25);
        } while (SystemClock.uptimeMillis() < until);
        throw new AssertionError("Insets marker did not reach its position: " + result);
    }

}
