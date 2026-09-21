package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.content.Intent;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.accessibility.AccessibilityNodeInfo;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import dev.cranpose.android.CranposeActivity;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.ArrayList;
import java.util.List;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

@RunWith(AndroidJUnit4.class)
public final class CranposeAccessibilityInteractionTest {
    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();

    @Test
    public void nativeActionsOperateTheSharedAccessibilityScreen() {
        Intent intent = new Intent(instrumentation.getTargetContext(), CranposeActivity.class)
                .putExtra("test_screen", "accessibility_robot");
        UiAutomation automation = instrumentation.getUiAutomation();
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(intent)) {
            scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(true));
            awaitNode(automation, "Accessibility robot");
            awaitNode(automation, "Account, Account");
            assertFalse(publishedText(automation).contains("robot-secret-value"));
            assertFalse(publishedText(automation).contains("Decorative secret"));
            String[] actions = {"Increase", "Remove", "Decrease"};
            int[] counts = {1, 2, 1};
            for (int index = 0; index < actions.length; index++) {
                assertTrue(actions[index], awaitNode(automation, actions[index])
                        .performAction(AccessibilityNodeInfo.ACTION_CLICK));
                awaitNode(automation, "Action count: " + counts[index]);
            }
            AccessibilityNodeInfo disabled = awaitNode(automation, "Disabled action");
            assertFalse(disabled.isEnabled());
            assertFalse(disabled.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_CLICK));
            assertFalse(disabled.performAction(AccessibilityNodeInfo.ACTION_CLICK));
            awaitNode(automation, "Action count: 1");
            AccessibilityNodeInfo loading = awaitNode(automation, "Loading");
            assertEquals("android.widget.ProgressBar", loading.getClassName());
            assertNotNull(loading.getRangeInfo());
            assertEquals(40.0f, loading.getRangeInfo().getCurrent(), 0.0f);
            assertFalse(loading.getActionList().contains(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS));
            Bundle range = new Bundle();
            range.putFloat(AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE, 70.0f);
            assertTrue(awaitNode(automation, "Volume")
                    .performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS.getId(), range));
            awaitNode(automation, "Volume value: 70");
            Bundle text = new Bundle();
            text.putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "Robot notes");
            assertTrue(awaitNode(automation, "Notes").performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, text));
            awaitNode(automation, "Edited: Robot notes");
            Bundle selection = new Bundle();
            selection.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, 2);
            selection.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, 7);
            assertTrue(awaitNode(automation, "Notes")
                    .performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, selection));
            long deadline = SystemClock.uptimeMillis() + 5000;
            AccessibilityNodeInfo field;
            do {
                field = awaitNode(automation, "Notes");
                if (field.getTextSelectionStart() == 2 && field.getTextSelectionEnd() == 7) break;
                SystemClock.sleep(50);
            } while (SystemClock.uptimeMillis() < deadline);
            assertEquals(2, field.getTextSelectionStart());
            assertEquals(7, field.getTextSelectionEnd());
            assertTrue(awaitNode(automation, "Express").isCheckable());
            assertTrue(awaitNode(automation, "Express").isChecked());
            assertFalse(awaitNode(automation, "Standard").isChecked());
            assertTrue(awaitNode(automation, "Standard").performAction(AccessibilityNodeInfo.ACTION_CLICK));
            long radioDeadline = SystemClock.uptimeMillis() + 5000;
            while (!awaitNode(automation, "Standard").isChecked() && SystemClock.uptimeMillis() < radioDeadline) {
                SystemClock.sleep(50);
            }
            assertTrue(awaitNode(automation, "Standard").isChecked());
            assertFalse(awaitNode(automation, "Express").isChecked());
            assertTrue(awaitNode(automation, "Open preferences").performAction(AccessibilityNodeInfo.ACTION_CLICK));
            awaitNode(automation, "Close preferences");
            assertFalse(publishedText(automation).contains("Increase"));
            assertTrue(awaitNode(automation, "Open confirmation").performAction(AccessibilityNodeInfo.ACTION_CLICK));
            awaitNode(automation, "Close confirmation");
            assertFalse(publishedText(automation).contains("Close preferences"));
            assertTrue(awaitNode(automation, "Close confirmation").performAction(AccessibilityNodeInfo.ACTION_CLICK));
            assertTrue(awaitNode(automation, "Close preferences").performAction(AccessibilityNodeInfo.ACTION_CLICK));
            awaitNode(automation, "Increase");
        }
    }

    private static AccessibilityNodeInfo awaitNode(UiAutomation automation, String label) {
        long deadline = SystemClock.uptimeMillis() + 10000;
        List<AccessibilityNodeInfo> nodes = new ArrayList<>();
        do {
            nodes.clear();
            collect(automation.getRootInActiveWindow(), nodes);
            for (AccessibilityNodeInfo node : nodes) {
                if (label.contentEquals(node.getContentDescription() == null ? "" : node.getContentDescription())) {
                    return node;
                }
            }
            SystemClock.sleep(50);
        } while (SystemClock.uptimeMillis() < deadline);
        throw new AssertionError("Missing native node " + label + "; published=" + publishedText(automation));
    }

    private static String publishedText(UiAutomation automation) {
        List<AccessibilityNodeInfo> nodes = new ArrayList<>();
        collect(automation.getRootInActiveWindow(), nodes);
        StringBuilder result = new StringBuilder();
        for (AccessibilityNodeInfo node : nodes) {
            result.append(node.getContentDescription()).append('|').append(node.getText()).append('\n');
        }
        return result.toString();
    }

    private static void collect(AccessibilityNodeInfo node, List<AccessibilityNodeInfo> nodes) {
        if (node == null) return;
        nodes.add(node);
        for (int index = 0; index < node.getChildCount(); index++) collect(node.getChild(index), nodes);
    }
}
