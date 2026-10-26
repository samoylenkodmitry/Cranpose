package com.compose_rs.demo;

import android.graphics.Bitmap;
import android.view.PointerIcon;
import android.view.View;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import dev.cranpose.android.CranposeActivity;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.assertEquals;

@RunWith(AndroidJUnit4.class)
public final class PointerIconTest {
    @Test public void activityDisplaysSystemAndRgbaCursors() {
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(CranposeActivity.class)) {
            scenario.onActivity(activity -> {
                View content = activity.findViewById(android.R.id.content);
                activity.cranposeSetPointerIcon(PointerIcon.TYPE_CROSSHAIR, 0, null, 0, 0, 0, 0);
                assertEquals(PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_CROSSHAIR),
                        content.getPointerIcon());
                byte[] rgba = {(byte)255, 0, 0, (byte)255, 0, (byte)255, 0, (byte)255,
                        0, 0, (byte)255, (byte)255, (byte)255, (byte)255, (byte)255, (byte)255};
                Bitmap bitmap = Bitmap.createBitmap(new int[] {0xffff0000, 0xff00ff00,
                        0xff0000ff, 0xffffffff}, 2, 2, Bitmap.Config.ARGB_8888);
                bitmap.setDensity(Bitmap.DENSITY_NONE);
                activity.cranposeSetPointerIcon(-1, 27, rgba, 2, 2, 1, 1);
                assertEquals(PointerIcon.create(bitmap, 1, 1), content.getPointerIcon());
                activity.cranposeSetPointerIcon(PointerIcon.TYPE_DEFAULT, 0, null, 0, 0, 0, 0);
                assertEquals(PointerIcon.getSystemIcon(activity, PointerIcon.TYPE_DEFAULT),
                        content.getPointerIcon());
            });
        }
    }
}
