package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.accessibilityservice.AccessibilityServiceInfo;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Rect;
import android.os.SystemClock;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import dev.cranpose.android.CranposeActivity;
import java.io.File;
import java.io.FileOutputStream;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
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
            await(automation, "Insets closed");
            Rect before = bounds(await(automation, "Insets bottom marker"));
            Rect field = bounds(await(automation, "Insets editor"));
            long time = SystemClock.uptimeMillis();
            for (int action : new int[] {MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP}) {
                MotionEvent event = MotionEvent.obtain(time, SystemClock.uptimeMillis(), action, field.centerX(), field.centerY(), 0);
                instrumentation.sendPointerSync(event);
                event.recycle();
            }
            await(automation, "Insets open");
            Rect after = awaitMarker(automation, before, true);
            assertTrue("Keyboard must lift the bottom marker", after.bottom < before.bottom - 100);
            Rect editor = bounds(await(automation, "Insets editor"));
            assertTrue("Focused editor stays above the bottom marker", editor.bottom <= after.bottom);
            screenshot(automation, "insets-open.png");
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK);
            await(automation, "Insets closed");
            Rect restored = awaitMarker(automation, before, false);
            assertEquals("Hiding the keyboard restores padding", before.bottom, restored.bottom);
            screenshot(automation, "insets-closed.png");
        }
    }

    private static Rect awaitMarker(UiAutomation automation, Rect before, boolean open) {
        long until = SystemClock.uptimeMillis() + 5000;
        Rect result;
        do {
            result = bounds(await(automation, "Insets bottom marker"));
            if (open ? result.bottom < before.bottom - 100 : result.bottom == before.bottom) return result;
            SystemClock.sleep(25);
        } while (SystemClock.uptimeMillis() < until);
        throw new AssertionError("Insets marker did not reach its position: " + result);
    }

    private void screenshot(UiAutomation automation, String name) throws Exception {
        Bitmap image = automation.takeScreenshot();
        assertNotNull(image);
        try (FileOutputStream out = new FileOutputStream(new File(instrumentation.getTargetContext().getExternalFilesDir(null), name))) {
            assertTrue(image.compress(Bitmap.CompressFormat.PNG, 100, out));
        } finally {
            image.recycle();
        }
    }

    private static Rect bounds(AccessibilityNodeInfo node) {
        Rect bounds = new Rect();
        node.getBoundsInScreen(bounds);
        return bounds;
    }

    private static AccessibilityNodeInfo await(UiAutomation automation, String label) {
        long until = SystemClock.uptimeMillis() + 10000;
        do {
            for (AccessibilityWindowInfo window : automation.getWindows()) {
                AccessibilityNodeInfo node = find(window.getRoot(), label);
                if (node != null) return node;
            }
            SystemClock.sleep(25);
        } while (SystemClock.uptimeMillis() < until);
        throw new AssertionError("Missing node: " + label);
    }

    private static AccessibilityNodeInfo find(AccessibilityNodeInfo node, String label) {
        if (node == null) return null;
        if (label.contentEquals(node.getContentDescription() == null ? "" : node.getContentDescription())) return node;
        for (int index = 0; index < node.getChildCount(); index++) {
            AccessibilityNodeInfo found = find(node.getChild(index), label);
            if (found != null) return found;
        }
        return null;
    }
}
