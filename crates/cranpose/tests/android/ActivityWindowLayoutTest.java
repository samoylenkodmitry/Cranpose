package com.compose_rs.demo;

import android.view.WindowManager;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import dev.cranpose.android.CranposeActivity;
import dev.cranpose.android.CranposeOverlayWindow;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.assertEquals;

@RunWith(AndroidJUnit4.class)
public final class ActivityWindowLayoutTest {
    @Test public void resizingThenReleasingRestoresTheSystemLayout() {
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(CranposeActivity.class)) {
            scenario.onActivity(activity -> {
                activity.getWindow().setLayout(WindowManager.LayoutParams.MATCH_PARENT,
                        WindowManager.LayoutParams.MATCH_PARENT);
                for (int pass = 0; pass < 3; pass++) {
                    CranposeOverlayWindow.setActivityWindowLayout(activity, 440, 660);
                    CranposeOverlayWindow.setActivityWindowLayout(activity, 550, 825);
                    assertEquals(550, activity.getWindow().getAttributes().width);
                    assertEquals(825, activity.getWindow().getAttributes().height);
                    CranposeOverlayWindow.restoreActivityWindowLayout(activity);
                    assertEquals(WindowManager.LayoutParams.MATCH_PARENT, activity.getWindow().getAttributes().width);
                    assertEquals(WindowManager.LayoutParams.MATCH_PARENT, activity.getWindow().getAttributes().height);
                    CranposeOverlayWindow.restoreActivityWindowLayout(activity);
                    assertEquals(WindowManager.LayoutParams.MATCH_PARENT, activity.getWindow().getAttributes().width);
                }
            });
        }
    }
}
