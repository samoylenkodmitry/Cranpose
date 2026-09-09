import android.os.SystemClock;
import android.view.InputEvent;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import java.lang.reflect.Method;

public final class CranposeBenchmarkRoute {
    private final Object manager;
    private final Method inject;

    private CranposeBenchmarkRoute() throws Exception {
        Class<?> type = Class.forName(android.os.Build.VERSION.SDK_INT >= 34
            ? "android.hardware.input.InputManagerGlobal" : "android.hardware.input.InputManager");
        manager = type.getMethod("getInstance").invoke(null);
        inject = type.getMethod("injectInputEvent", InputEvent.class, int.class);
    }

    private void send(InputEvent event) throws Exception {
        if (!(Boolean) inject.invoke(manager, event, 0)) {
            throw new IllegalStateException("Input injection refused");
        }
    }

    private void touch(long down, int action, float x, float y) throws Exception {
        MotionEvent event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x, y, 0);
        try {
            event.setSource(InputDevice.SOURCE_TOUCHSCREEN);
            send(event);
        } finally {
            event.recycle();
        }
    }

    private static void waitUntil(long deadline) {
        long remaining = deadline - SystemClock.uptimeMillis();
        if (remaining > 0) SystemClock.sleep(remaining);
    }

    private static void command(String... command) throws Exception {
        if (new ProcessBuilder(command).inheritIO().start().waitFor() != 0) {
            throw new IllegalStateException("Device command failed");
        }
    }

    private void gesture(float x, float y1, float y2, int duration) throws Exception {
        send(new KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_WAKEUP));
        send(new KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_WAKEUP));
        long down = SystemClock.uptimeMillis();
        touch(down, MotionEvent.ACTION_DOWN, x, y1);
        try {
            for (int elapsed = 16; elapsed < duration; elapsed += 16) {
                waitUntil(down + elapsed);
                float fraction = Math.min(1f, (SystemClock.uptimeMillis() - down) / (float) duration);
                touch(down, MotionEvent.ACTION_MOVE, x, y1 + (y2 - y1) * fraction);
            }
            waitUntil(down + duration);
            touch(down, MotionEvent.ACTION_UP, x, y2);
        } catch (Exception error) {
            touch(down, MotionEvent.ACTION_CANCEL, x, y2);
            throw error;
        }
    }

    public static void main(String[] args) throws Exception {
        float x = Float.parseFloat(args[0]);
        float y1 = Float.parseFloat(args[1]);
        float y2 = Float.parseFloat(args[2]);
        int count = Integer.parseInt(args[3]);
        int duration = Integer.parseInt(args[4]);
        int period = Integer.parseInt(args[5]);
        int window = Integer.parseInt(args[6]);
        if (count < 0 || duration < 1 || period < duration || window < 1 || (long) count * period > window) {
            throw new IllegalArgumentException("Invalid measurement timing");
        }
        CranposeBenchmarkRoute driver = new CranposeBenchmarkRoute();
        System.out.println("route_pid=" + android.os.Process.myPid() + " run_id=" + args[7]);
        System.out.flush();
        command("dumpsys", "SurfaceFlinger", "--timestats", "-clear", "-enable");
        long elapsed;
        try {
            long start = SystemClock.uptimeMillis();
            for (int step = 0; step < count; step++) {
                waitUntil(start + (long) step * period);
                long gestureStart = SystemClock.uptimeMillis();
                driver.gesture(x, y1, y2, duration);
                System.out.println("gesture=" + step + " start=" + (gestureStart - start)
                    + " end=" + (SystemClock.uptimeMillis() - start));
            }
            waitUntil(start + window);
            elapsed = SystemClock.uptimeMillis() - start;
        } finally {
            command("dumpsys", "SurfaceFlinger", "--timestats", "-disable");
        }
        System.out.println("elapsed_ms=" + elapsed);
        System.out.println("SF_BEGIN");
        command("dumpsys", "SurfaceFlinger", "--timestats", "-dump");
        System.out.println("SF_END");
    }
}
