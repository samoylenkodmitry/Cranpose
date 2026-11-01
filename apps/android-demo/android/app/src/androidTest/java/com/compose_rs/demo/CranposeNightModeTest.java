package com.compose_rs.demo;

import android.app.Instrumentation;
import android.app.UiAutomation;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.os.ParcelFileDescriptor;
import android.os.SystemClock;

import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.platform.app.InstrumentationRegistry;

import dev.cranpose.android.CranposeActivity;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;

import static org.junit.Assert.assertTrue;

@RunWith(AndroidJUnit4.class)
public final class CranposeNightModeTest {
    private static final long TIMEOUT_MS = 10_000;
    // The demo palette's page backgrounds are near white and near black.
    private static final double LIGHT_PAGE = 0.8;
    private static final double DARK_PAGE = 0.2;

    private final Instrumentation instrumentation = InstrumentationRegistry.getInstrumentation();

    @Test
    public void aRunningPageFollowsTheSystemIntoNightModeAndBack() throws IOException {
        UiAutomation automation = instrumentation.getUiAutomation();
        String original = shell(automation, "cmd uimode night").replace("Night mode:", "").trim();
        shell(automation, "cmd uimode night no");
        Intent intent = new Intent(instrumentation.getTargetContext(), CranposeActivity.class)
                .putExtra("tab", "animations");
        try (ActivityScenario<CranposeActivity> scenario = ActivityScenario.launch(intent)) {
            scenario.onActivity(activity -> activity.cranposeSetKeepScreenOn(true));
            assertTrue("the page never showed light", awaitPage(automation, true));
            shell(automation, "cmd uimode night yes");
            assertTrue("the page stayed light after the system turned night mode on",
                    awaitPage(automation, false));
            shell(automation, "cmd uimode night no");
            assertTrue("the page stayed dark after the system turned night mode off",
                    awaitPage(automation, true));
        } finally {
            shell(automation, "cmd uimode night " + original);
        }
    }

    private static boolean awaitPage(UiAutomation automation, boolean light) {
        long deadline = SystemClock.uptimeMillis() + TIMEOUT_MS;
        while (SystemClock.uptimeMillis() < deadline) {
            Bitmap screen = automation.takeScreenshot();
            if (screen != null) {
                double luminance = medianLuminance(screen);
                screen.recycle();
                if (light ? luminance > LIGHT_PAGE : luminance < DARK_PAGE) {
                    return true;
                }
            }
            SystemClock.sleep(200);
        }
        return false;
    }

    // The middle of the screen, sampled on a coarse grid, so text and
    // animated shapes on the page do not move the result.
    private static double medianLuminance(Bitmap screen) {
        int width = screen.getWidth();
        int height = screen.getHeight();
        int step = 16;
        int columns = (width * 6 / 10) / step;
        int rows = (height * 6 / 10) / step;
        double[] samples = new double[columns * rows];
        int index = 0;
        for (int row = 0; row < rows; row++) {
            for (int column = 0; column < columns; column++) {
                int pixel = screen.getPixel(width / 5 + column * step, height / 5 + row * step);
                samples[index++] = (0.2126 * Color.red(pixel)
                        + 0.7152 * Color.green(pixel)
                        + 0.0722 * Color.blue(pixel)) / 255.0;
            }
        }
        Arrays.sort(samples);
        return samples[samples.length / 2];
    }

    private static String shell(UiAutomation automation, String command) throws IOException {
        ParcelFileDescriptor output = automation.executeShellCommand(command);
        try (InputStream input = new ParcelFileDescriptor.AutoCloseInputStream(output)) {
            ByteArrayOutputStream text = new ByteArrayOutputStream();
            byte[] buffer = new byte[256];
            for (int read; (read = input.read(buffer)) != -1; ) {
                text.write(buffer, 0, read);
            }
            return text.toString(StandardCharsets.UTF_8.name());
        }
    }
}
