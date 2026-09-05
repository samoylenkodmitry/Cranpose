package com.compose_rs.demo;

import android.view.WindowManager;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import dev.cranpose.android.CranposeActivity;

import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.concurrent.TimeUnit;

import static org.junit.Assert.assertEquals;

@RunWith(AndroidJUnit4.class)
public final class CranposeActivityKeepScreenOnTest {

    @Test
    public void testWorkerThreadCanToggleKeepScreenOn() throws Exception {
        try (ActivityScenario<CranposeActivity> scenario =
                     ActivityScenario.launch(CranposeActivity.class)) {
            final CranposeActivity[] holder = new CranposeActivity[1];
            scenario.onActivity(activity -> holder[0] = activity);
            runWorkerTransition(holder[0], true);
            assertFlag(scenario, true);
            runWorkerTransition(holder[0], false);
            assertFlag(scenario, false);
            scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(true));
            assertFlag(scenario, true);
            scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(false));
            assertFlag(scenario, false);
        }
    }

    private void runWorkerTransition(CranposeActivity activity, boolean enabled)
            throws Exception {
        java.util.concurrent.FutureTask<Void> worker = new java.util.concurrent.FutureTask<>(() -> {
            activity.cranposeSetKeepScreenOn(enabled);
            return null;
        });
        new Thread(worker, "keep-screen-on-test").start();
        worker.get(5, TimeUnit.SECONDS);
        InstrumentationRegistry.getInstrumentation().waitForIdleSync();
    }

    private void assertFlag(ActivityScenario<CranposeActivity> scenario, boolean expected) {
        final boolean[] actual = new boolean[1];
        scenario.onActivity(activity -> actual[0] =
                (activity.getWindow().getAttributes().flags
                        & WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) != 0);
        assertEquals(expected, actual[0]);
    }
}
