package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.content.Intent;
import android.os.SystemClock;
import android.view.accessibility.AccessibilityNodeInfo;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import com.google.android.apps.common.testing.accessibility.framework.AccessibilityCheckPreset;
import com.google.android.apps.common.testing.accessibility.framework.AccessibilityCheckResult.AccessibilityCheckResultType;
import com.google.android.apps.common.testing.accessibility.framework.AccessibilityCheckResultUtils;
import com.google.android.apps.common.testing.accessibility.framework.AccessibilityHierarchyCheck;
import com.google.android.apps.common.testing.accessibility.framework.AccessibilityHierarchyCheckResult;
import com.google.android.apps.common.testing.accessibility.framework.uielement.AccessibilityHierarchyAndroid;

import dev.cranpose.android.CranposeActivity;

import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Set;

import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

/**
 * Runs Google's Accessibility Test Framework over the node tree the
 * Cranpose activity publishes, the same checks the Accessibility Scanner app
 * and Espresso's accessibility validator run: a name on every control, no two
 * controls with one name, a touch target of 48 dp, and text contrast.
 */
@RunWith(AndroidJUnit4.class)
public final class CranposeAccessibilityAuditTest {
    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();

    @Test
    public void thePublishedTreePassesTheAccessibilityTestFramework() {
        UiAutomation automation = instrumentation.getUiAutomation();
        Intent intent = new Intent(instrumentation.getTargetContext(), CranposeActivity.class)
                .putExtra("test_screen", "accessibility_navigation");
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(intent)) {
            scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(true));
            AccessibilityNodeInfo root = awaitRootWith(automation, "Library page");
            List<AccessibilityHierarchyCheckResult> errors = errorsIn(root);
            if (!errors.isEmpty()) {
                StringBuilder report = new StringBuilder("The accessibility checks found:\n");
                for (AccessibilityHierarchyCheckResult error : errors) {
                    report.append("  ")
                            .append(error.getSourceCheckClass().getSimpleName())
                            .append(": ")
                            .append(error.getMessage(Locale.getDefault()))
                            .append('\n');
                }
                fail(report.toString());
            }
        }
    }

    private List<AccessibilityHierarchyCheckResult> errorsIn(AccessibilityNodeInfo root) {
        AccessibilityHierarchyAndroid hierarchy = AccessibilityHierarchyAndroid
                .newBuilder(root, instrumentation.getTargetContext())
                .build();
        Set<AccessibilityHierarchyCheck> checks =
                AccessibilityCheckPreset.getAccessibilityHierarchyChecksForPreset(
                        AccessibilityCheckPreset.LATEST);
        List<AccessibilityHierarchyCheckResult> results = new ArrayList<>();
        for (AccessibilityHierarchyCheck check : checks) {
            results.addAll(check.runCheckOnHierarchy(hierarchy));
        }
        return AccessibilityCheckResultUtils.getResultsForType(
                results, AccessibilityCheckResultType.ERROR);
    }

    private static AccessibilityNodeInfo awaitRootWith(UiAutomation automation, String text) {
        long deadline = SystemClock.uptimeMillis() + 10_000;
        while (SystemClock.uptimeMillis() < deadline) {
            AccessibilityNodeInfo root = automation.getRootInActiveWindow();
            if (root != null && !root.findAccessibilityNodeInfosByText(text).isEmpty()) {
                return root;
            }
            SystemClock.sleep(100);
        }
        assertTrue("The screen never published " + text, false);
        return null;
    }
}
