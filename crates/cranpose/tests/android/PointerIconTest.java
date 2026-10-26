package com.compose_rs.demo;

import android.view.PointerIcon;
import android.view.View;
import android.view.MotionEvent;
import android.view.InputDevice;
import android.os.SystemClock;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import dev.cranpose.android.CranposeActivity;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNotEquals;

@RunWith(AndroidJUnit4.class)
public final class PointerIconTest {
    @Test public void activitySwitchesSystemAndCustomCursors() {
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(CranposeActivity.class)) {
            scenario.onActivity(activity -> {
                View content = activity.findViewById(android.R.id.content);
                activity.cranposeSetPointerIcon(PointerIcon.TYPE_CROSSHAIR, 0, null, 0, 0, 0, 0);
                assertEquals(PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_CROSSHAIR),
                        content.getPointerIcon());
                byte[] rgba = {(byte)255, 0, 0, (byte)255, 0, (byte)255, 0, (byte)255,
                        0, 0, (byte)255, (byte)255, (byte)255, (byte)255, (byte)255, (byte)255};
                activity.cranposeSetPointerIcon(-1, 27, rgba, 2, 2, 1, 1);
                assertNotNull(content.getPointerIcon());
                assertNotEquals(PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_CROSSHAIR),
                        content.getPointerIcon());
                activity.cranposeSetPointerIcon(PointerIcon.TYPE_DEFAULT, 0, null, 0, 0, 0, 0);
                assertEquals(PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_DEFAULT),
                        content.getPointerIcon());
            });
        }
    }
    @Test public void stylusHoverResolvesTheSharedCursor() {
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(CranposeActivity.class)) {
            scenario.onActivity(activity -> activity.cranposeSetPointerIcon(
                    PointerIcon.TYPE_CROSSHAIR, 0, null, 0, 0, 0, 0));
            final boolean[] resolved = {false};
            for (int attempt = 0; attempt < 30 && !resolved[0]; attempt++) {
                scenario.onActivity(activity -> {
                    View content = activity.findViewById(android.R.id.content);
                    if (content.getWidth() == 0 || content.getHeight() == 0) return;
                    int[] location = new int[2];
                    content.getLocationInWindow(location);
                    MotionEvent.PointerProperties property = new MotionEvent.PointerProperties();
                    property.id = 0;
                    property.toolType = MotionEvent.TOOL_TYPE_STYLUS;
                    MotionEvent.PointerCoords position = new MotionEvent.PointerCoords();
                    position.x = location[0] + content.getWidth() / 2f;
                    position.y = location[1] + content.getHeight() / 2f;
                    long now = SystemClock.uptimeMillis();
                    MotionEvent event = MotionEvent.obtain(now, now, MotionEvent.ACTION_HOVER_MOVE,
                            1, new MotionEvent.PointerProperties[]{property},
                            new MotionEvent.PointerCoords[]{position}, 0, 0, 1, 1, 0, 0,
                            InputDevice.SOURCE_STYLUS, 0);
                    try {
                        resolved[0] = PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_CROSSHAIR)
                                .equals(activity.getWindow().getDecorView().onResolvePointerIcon(event, 0));
                    } finally {
                        event.recycle();
                    }
                });
                if (!resolved[0]) SystemClock.sleep(100);
            }
            assertTrue("Stylus hover must resolve the selected cursor", resolved[0]);
        }
    }

}
