package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.graphics.Rect;
import android.os.SystemClock;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.accessibility.AccessibilityNodeInfo;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import dev.cranpose.android.CranposeActivity;

import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.ArrayList;
import java.util.List;

import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

@RunWith(AndroidJUnit4.class)
public final class CranposeAccessibilityNavigationTest {
    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();

    @Test
    public void reconnectPublishesThePageRenderedWhileAccessibilityWasDisabled() throws Exception {
        UiAutomation automation = instrumentation.getUiAutomation();
        Context context = instrumentation.getTargetContext();
        try (ActivityScenario<CranposeActivity> scenario = launchScreen()) {
            Rect target = awaitPage(automation, "Library page", "Settings page");
            automation = instrumentation.getUiAutomation(UiAutomation.FLAG_DONT_USE_ACCESSIBILITY);
            awaitAccessibilityDisabled(context);
            tap(target.centerX(), target.centerY());
            SystemClock.sleep(500);
            Bitmap frame = automation.takeScreenshot();
            assertNotNull("The rendered page must be captured", frame);
            try {
                int background = frame.getPixel(frame.getWidth() / 2, frame.getHeight() / 4);
                assertTrue("Settings must render green before its hierarchy is checked",
                        Color.green(background) > 240 && Color.blue(background) < 15);
            } finally {
                frame.recycle();
            }
            automation = instrumentation.getUiAutomation();
            awaitPage(automation, "Settings page", "Library page");
        }
    }

    @Test
    public void connectedAccessibilityTracksPageChangesInBothDirections() {
        UiAutomation automation = instrumentation.getUiAutomation();
        try (ActivityScenario<CranposeActivity> scenario = launchScreen()) {
            for (String expected : new String[] {"Settings page", "Library page"}) {
                String previous = expected.equals("Settings page") ? "Library page" : "Settings page";
                Rect target = awaitPage(automation, previous, expected);
                tap(target.centerX(), target.centerY());
                awaitPage(automation, expected, previous);
            }
        }
    }

    private ActivityScenario<CranposeActivity> launchScreen() {
        Intent intent = new Intent(instrumentation.getTargetContext(), CranposeActivity.class)
                .putExtra("test_screen", "accessibility_navigation");
        ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(intent);
        scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(true));
        return scenario;
    }

    private static void awaitAccessibilityDisabled(Context context) {
        AccessibilityManager manager = context.getSystemService(AccessibilityManager.class);
        long deadline = SystemClock.uptimeMillis() + 3000;
        while (manager.isEnabled() && SystemClock.uptimeMillis() < deadline) {
            SystemClock.sleep(25);
        }
        assertFalse("The test must navigate with accessibility disconnected", manager.isEnabled());
    }

    private Rect awaitPage(UiAutomation automation, String expected, String absent) {
        long deadline = SystemClock.uptimeMillis() + 3000;
        List<String> labels = new ArrayList<>();
        do {
            labels.clear();
            AccessibilityNodeInfo root = automation.getRootInActiveWindow();
            Rect bounds = visit(root, expected, labels);
            if (bounds != null && !labels.contains(absent)) return bounds;
            SystemClock.sleep(50);
        } while (SystemClock.uptimeMillis() < deadline);
        throw new AssertionError("Expected " + expected + " without " + absent + "; hierarchy=" + labels);
    }

    private static Rect visit(AccessibilityNodeInfo node, String expected, List<String> labels) {
        if (node == null) return null;
        Rect found = null;
        CharSequence description = node.getContentDescription();
        String label = description == null ? "" : description.toString();
        labels.add(label);
        if (label.equals(expected)) {
            Rect bounds = new Rect();
            node.getBoundsInScreen(bounds);
            if (!bounds.isEmpty()) found = bounds;
        }
        for (int index = 0; index < node.getChildCount(); index++) {
            Rect child = visit(node.getChild(index), expected, labels);
            if (child != null) found = child;
        }
        return found;
    }

    private void tap(float x, float y) {
        long down = SystemClock.uptimeMillis();
        for (int action : new int[] {MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP}) {
            MotionEvent event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x, y, 0);
            try {
                instrumentation.sendPointerSync(event);
            } finally {
                event.recycle();
            }
        }
    }
}
