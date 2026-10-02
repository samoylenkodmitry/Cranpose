package com.compose_rs.demo;

import android.app.Instrumentation;
import android.content.Intent;
import android.os.SystemClock;
import android.view.View;
import android.view.ViewGroup;
import android.view.accessibility.AccessibilityNodeInfo;
import android.webkit.WebView;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;
import dev.cranpose.android.CranposeActivity;
import org.junit.Test;
import org.junit.runner.RunWith;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.BooleanSupplier;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
@androidx.test.filters.SdkSuppress(minSdkVersion = 29)
public final class CranposeWebViewTabTest {
    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();

    @Test public void websiteSurvivesRecompositionAndIsDisposedWithItsSlot() throws Exception {
        android.accessibilityservice.AccessibilityServiceInfo service = instrumentation.getUiAutomation().getServiceInfo();
        service.flags |= android.accessibilityservice.AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS;
        instrumentation.getUiAutomation().setServiceInfo(service);
        Intent intent = new Intent(instrumentation.getTargetContext(), CranposeActivity.class).putExtra("tab", "webview");
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(intent)) {
            await(() -> findNode("Loaded https://example.com") != null);
            await(() -> findNode("Learn more") != null);
            WebView original = browser(scenario);
            assertNotNull(original);
            assertEquals("\"preserved\"", evaluate(original, "window.cranposeMarker = 'preserved'"));
            click("Cranpose: 0");
            await(() -> findNode("Cranpose: 1") != null);
            assertSame(original, browser(scenario));
            assertEquals("\"preserved\"", evaluate(original, "window.cranposeMarker"));
            assertTrue(evaluate(original, "document.body.innerText").contains("Learn more"));
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED);
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED);
            assertEquals("\"preserved\"", evaluate(original, "window.cranposeMarker"));
            click("Remove WebView");
            await(() -> browser(scenario) == null);
            click("Show WebView");
            await(() -> findNode("Loaded https://example.com") != null && browser(scenario) != null);
            assertNotSame(original, browser(scenario));
        }
    }

    private String evaluate(WebView view, String script) throws Exception {
        CountDownLatch done = new CountDownLatch(1);
        AtomicReference<String> value = new AtomicReference<>();
        instrumentation.runOnMainSync(() -> view.evaluateJavascript(script, result -> { value.set(result); done.countDown(); }));
        assertTrue(done.await(10, TimeUnit.SECONDS));
        return value.get();
    }

    private WebView browser(ActivityScenario<CranposeActivity> scenario) {
        AtomicReference<WebView> found = new AtomicReference<>();
        scenario.onActivity(activity -> {
            for (View root : android.view.inspector.WindowInspector.getGlobalWindowViews()) {
                WebView web = browser(root);
                if (web != null) { found.set(web); break; }
            }
        });
        return found.get();
    }

    private WebView browser(View view) {
        if (view instanceof WebView) return (WebView) view;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                WebView found = browser(group.getChildAt(i));
                if (found != null) return found;
            }
        }
        return null;
    }

    private AccessibilityNodeInfo findNode(String text) {
        for (android.view.accessibility.AccessibilityWindowInfo window : instrumentation.getUiAutomation().getWindows()) {
            AccessibilityNodeInfo found = findNode(window.getRoot(), text);
            if (found != null) return found;
        }
        return null;
    }

    private AccessibilityNodeInfo findNode(AccessibilityNodeInfo node, String text) {
        if (node == null) return null;
        if ((node.getText() != null && node.getText().toString().startsWith(text)) ||
            (node.getContentDescription() != null && node.getContentDescription().toString().startsWith(text))) return node;
        for (int i = 0; i < node.getChildCount(); i++) {
            AccessibilityNodeInfo found = findNode(node.getChild(i), text);
            if (found != null) return found;
        }
        return null;
    }

    private void click(String text) {
        await(() -> findNode(text) != null);
        assertTrue(findNode(text).performAction(AccessibilityNodeInfo.ACTION_CLICK));
    }

    private void await(BooleanSupplier condition) {
        long deadline = SystemClock.uptimeMillis() + 30000;
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition.getAsBoolean()) return;
            SystemClock.sleep(50);
        }
        throw new AssertionError("WebView tab did not reach the expected state");
    }
}
