package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.accessibilityservice.AccessibilityServiceInfo;
import android.graphics.Rect;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.KeyCharacterMap;
import android.view.KeyEvent;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityWindowInfo;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import java.io.File;
import java.io.FileOutputStream;
import android.os.ParcelFileDescriptor;
import java.nio.charset.StandardCharsets;
import org.json.JSONArray;
import org.json.JSONObject;
import org.junit.Test;
import org.junit.Assume;
import static com.compose_rs.demo.ImeTestSupport.*;
import org.junit.runner.RunWith;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

@RunWith(AndroidJUnit4.class)
public final class CranposeImeAppsTest {
    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();
    private UiAutomation automation;
    private File output;

    @Test
    public void appFieldsRemainVisibleWithKeyboard() throws Exception {
        Bundle arguments = InstrumentationRegistry.getArguments();
        Assume.assumeTrue(arguments.containsKey("app_package"));
        automation = instrumentation.getUiAutomation();
        AccessibilityServiceInfo info = automation.getServiceInfo();
        info.flags |= AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        automation.setServiceInfo(info);
        StringBuilder launch = new StringBuilder("am start -W -n ")
                .append(launchToken(arguments.getString("app_package") + "/dev.cranpose.android.CranposeActivity"));
        JSONObject extras = new JSONObject(arguments.getString("extras", "{}"));
        java.util.Iterator<String> keys = extras.keys();
        while (keys.hasNext()) {
            String key = keys.next();
            launch.append(" --es ").append(launchToken(key)).append(" ").append(launchToken(extras.getString(key)));
        }
        runShell(launch.toString());
        output = new File(instrumentation.getTargetContext().getExternalFilesDir(null), arguments.getString("run_id", "ime-apps"));
        output.mkdirs();
        JSONArray steps = new JSONArray(arguments.getString("steps", "[]"));
        for (int index = 0; index < steps.length(); index++) {
            JSONObject step = steps.getJSONObject(index);
            String name = String.format("%02d-%s", index, step.getString("action"));
            try {
                act(step);
                capture(name);
            } catch (Throwable error) {
                capture(name + "-failed");
                throw error;
            }
        }
    }

    private static String launchToken(String text) {
        if (!text.matches("[A-Za-z0-9_./:-]+")) throw new IllegalArgumentException("Unsupported launch argument: " + text);
        return text;
    }

    private void runShell(String command) throws Exception {
        try (ParcelFileDescriptor.AutoCloseInputStream result = new ParcelFileDescriptor.AutoCloseInputStream(
                automation.executeShellCommand(command))) {
            byte[] buffer = new byte[1024];
            while (result.read(buffer) != -1) { }
        }
    }

    private void act(JSONObject step) throws Exception {
        switch (step.getString("action")) {
            case "await": awaitNode(automation, step.getString("label")); break;
            case "reveal": {
                Rect screen = new Rect();
                for (AccessibilityWindowInfo window : automation.getWindows()) {
                    if (window.getType() == AccessibilityWindowInfo.TYPE_APPLICATION) window.getBoundsInScreen(screen);
                }
                for (int attempt = 0; attempt < 12; attempt++) {
                    AccessibilityNodeInfo node = findNode(automation, step.getString("label"));
                    Rect rect = node == null ? new Rect() : bounds(node);
                    if (!rect.isEmpty() && rect.top >= screen.top + 140 && rect.bottom < screen.bottom - 160) return;
                    boolean above = !rect.isEmpty() && rect.top < screen.top + 140;
                    int high = screen.top + screen.height() / 3;
                    int low = screen.top + screen.height() * 2 / 3;
                    touch(screen.centerX(), above ? high : low, screen.centerX(), above ? low : high, 450);
                    SystemClock.sleep(150);
                }
                throw new AssertionError("Could not scroll field into view: " + step.getString("label"));
            }
            case "tap": {
                Rect rect = bounds(awaitNode(automation, step.getString("label")));
                touch(rect.centerX(), rect.centerY(), rect.centerX(), rect.centerY(), 70);
                break;
            }
            case "touch":
                touch(step.getInt("x"), step.getInt("y"), step.getInt("x"), step.getInt("y"), 70);
                break;
            case "swipe":
                touch(step.getInt("x"), step.getInt("y"), step.getInt("x"), step.getInt("to_y"), 450);
                break;
            case "type":
                KeyEvent[] events = KeyCharacterMap.load(KeyCharacterMap.VIRTUAL_KEYBOARD)
                        .getEvents(step.getString("text").toCharArray());
                assertNotNull(events);
                for (KeyEvent event : events) assertTrue("Key injection failed", automation.injectInputEvent(event, true));
                break;
            case "back":
                runShell("input keyevent KEYCODE_BACK");
                break;
            case "visible": {
                Rect keyboard = awaitKeyboard(automation, true);
                long until = SystemClock.uptimeMillis() + 5000;
                Rect field;
                do {
                    field = bounds(awaitNode(automation, step.getString("label")));
                    if (!field.isEmpty() && field.top >= 0 && field.bottom <= keyboard.top) return;
                    SystemClock.sleep(25);
                } while (SystemClock.uptimeMillis() < until);
                throw new AssertionError("Field " + field + " overlaps keyboard " + keyboard);
            }
            case "closed": awaitKeyboard(automation, false); break;
            case "capture": break;
            default: throw new IllegalArgumentException(step.toString());
        }
    }

    private void touch(int x, int y, int endX, int endY, int duration) {
        long start = SystemClock.uptimeMillis();
        for (int index = 0; index <= 10; index++) {
            int action = index == 0 ? MotionEvent.ACTION_DOWN : index == 10 ? MotionEvent.ACTION_UP : MotionEvent.ACTION_MOVE;
            MotionEvent event = MotionEvent.obtain(start, SystemClock.uptimeMillis(), action,
                    x + (endX - x) * index / 10f, y + (endY - y) * index / 10f, 0);
            event.setSource(InputDevice.SOURCE_TOUCHSCREEN);
            assertTrue("Touch injection failed", automation.injectInputEvent(event, true));
            event.recycle();
            if (index < 10) SystemClock.sleep(duration / 10);
        }
    }

    private void capture(String name) throws Exception {
        screenshot(automation, new File(output, name + ".png"));
        JSONArray windows = new JSONArray();
        for (AccessibilityWindowInfo window : automation.getWindows()) {
            JSONObject item = new JSONObject();
            Rect rect = new Rect();
            window.getBoundsInScreen(rect);
            item.put("type", window.getType()).put("bounds", rect.toShortString());
            item.put("root", describe(window.getRoot()));
            windows.put(item);
        }
        try (FileOutputStream stream = new FileOutputStream(new File(output, name + ".json"))) {
            stream.write(windows.toString(2).getBytes(StandardCharsets.UTF_8));
        }
    }

    private static JSONObject describe(AccessibilityNodeInfo node) throws Exception {
        if (node == null) return null;
        JSONObject result = new JSONObject();
        result.put("text", node.getText()).put("label", node.getContentDescription())
                .put("bounds", bounds(node).toShortString()).put("focused", node.isFocused())
                .put("editable", node.isEditable()).put("selection", node.getTextSelectionStart());
        JSONArray nodes = new JSONArray();
        for (int index = 0; index < node.getChildCount(); index++) nodes.put(describe(node.getChild(index)));
        result.put("nodes", nodes);
        return result;
    }
}
