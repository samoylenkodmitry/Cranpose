package com.compose_rs.demo;

import android.app.UiAutomation;
import android.graphics.Bitmap;
import android.graphics.Rect;
import android.os.SystemClock;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;
import java.io.File;
import java.io.FileOutputStream;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

final class ImeTestSupport {
    private ImeTestSupport() { }

    static Rect bounds(AccessibilityNodeInfo node) {
        Rect rect = new Rect();
        node.getBoundsInScreen(rect);
        return rect;
    }

    static AccessibilityNodeInfo awaitNode(UiAutomation automation, String label) {
        long until = SystemClock.uptimeMillis() + 6000;
        do {
            AccessibilityNodeInfo found = findNode(automation, label);
            if (found != null) return found;
            SystemClock.sleep(25);
        } while (SystemClock.uptimeMillis() < until);
        throw new AssertionError("Missing node: " + label);
    }

    static AccessibilityNodeInfo findNode(UiAutomation automation, String label) {
        for (AccessibilityWindowInfo window : automation.getWindows()) {
            if (window.getType() != AccessibilityWindowInfo.TYPE_APPLICATION) continue;
            AccessibilityNodeInfo found = find(window.getRoot(), label);
            if (found != null) return found;
        }
        return null;
    }

    private static AccessibilityNodeInfo find(AccessibilityNodeInfo node, String label) {
        if (node == null) return null;
        if (label.contentEquals(node.getContentDescription() == null ? "" : node.getContentDescription())
                || label.contentEquals(node.getText() == null ? "" : node.getText())) return node;
        for (int index = 0; index < node.getChildCount(); index++) {
            AccessibilityNodeInfo found = find(node.getChild(index), label);
            if (found != null) return found;
        }
        return null;
    }

    static Rect awaitKeyboard(UiAutomation automation, boolean open) {
        long until = SystemClock.uptimeMillis() + 6000;
        long stableSince = 0;
        Rect previous = null;
        do {
            Rect current = null;
            for (AccessibilityWindowInfo window : automation.getWindows()) {
                if (window.getType() == AccessibilityWindowInfo.TYPE_INPUT_METHOD) {
                    current = new Rect();
                    window.getBoundsInScreen(current);
                }
            }
            boolean matches = open ? current != null && !current.isEmpty() : current == null;
            boolean same = current == null ? previous == null : current.equals(previous);
            if (matches && same) {
                if (stableSince == 0) stableSince = SystemClock.uptimeMillis();
                if (SystemClock.uptimeMillis() - stableSince >= 500) return current;
            } else stableSince = 0;
            previous = current;
            SystemClock.sleep(25);
        } while (SystemClock.uptimeMillis() < until);
        throw new AssertionError("Keyboard did not settle: open=" + open + " bounds=" + previous);
    }

    static void screenshot(UiAutomation automation, File file) throws Exception {
        Bitmap bitmap = automation.takeScreenshot();
        assertNotNull(bitmap);
        try (FileOutputStream stream = new FileOutputStream(file)) {
            assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream));
        } finally { bitmap.recycle(); }
    }
}
