package dev.cranpose.android;

import android.app.Activity;
import android.app.Application;
import android.content.Context;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.os.Bundle;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Process;

/**
 * The rotation of the device, behind {@code cranpose_services::device_motion}.
 *
 * <p>An application carries this class only when its build script declares
 * {@code Use::device_motion()}; from that declaration the Cranpose Gradle
 * plugin adds this source directory and the {@code HIGH_SAMPLING_RATE_SENSORS}
 * permission, and nothing of it reaches an application that does not declare
 * it.
 *
 * <p>Samples reach Rust on a thread of this class, not on the main thread, so
 * a busy interface does not delay them. The sensor runs only while Rust has a
 * reader for it and the activity is in front; a paused activity releases it
 * and takes it back on resume.
 */
public final class CranposeDeviceMotion implements SensorEventListener {
    /** The heading of the reference frame is arbitrary. */
    static final int ARBITRARY_HEADING = 0;
    /** The y axis of the reference frame points to magnetic north. */
    static final int MAGNETIC_NORTH = 1;

    private static native void nativeOnDeviceMotion(
            float x, float y, float z, float w, long timestampNs, int reference);

    private static native void nativeOnDeviceMotionInterrupted();

    private static final Object LOCK = new Object();
    private static CranposeDeviceMotion session;

    private final Activity activity;
    private final SensorManager sensors;
    private final HandlerThread thread;
    private final Handler handler;
    private final Application.ActivityLifecycleCallbacks lifecycle;
    private Sensor sensor;
    private int reference;
    private int periodMicros;
    private boolean listening;

    private CranposeDeviceMotion(Activity activity, SensorManager sensors) {
        this.activity = activity;
        this.sensors = sensors;
        this.thread = new HandlerThread(
                "cranpose-device-motion", Process.THREAD_PRIORITY_URGENT_DISPLAY);
        this.thread.start();
        this.handler = new Handler(thread.getLooper());
        this.lifecycle = new Application.ActivityLifecycleCallbacks() {
            @Override
            public void onActivityResumed(Activity resumed) {
                if (resumed == CranposeDeviceMotion.this.activity) {
                    synchronized (LOCK) {
                        listen();
                    }
                }
            }

            @Override
            public void onActivityPaused(Activity paused) {
                if (paused == CranposeDeviceMotion.this.activity) {
                    synchronized (LOCK) {
                        release();
                    }
                    nativeOnDeviceMotionInterrupted();
                }
            }

            @Override
            public void onActivityCreated(Activity created, Bundle state) {}

            @Override
            public void onActivityStarted(Activity started) {}

            @Override
            public void onActivityStopped(Activity stopped) {}

            @Override
            public void onActivitySaveInstanceState(Activity saved, Bundle state) {}

            @Override
            public void onActivityDestroyed(Activity destroyed) {}
        };
    }

    // The sensor for a reference. The arbitrary heading takes the game
    // rotation vector and, on a device without it, the magnetic one.
    private static Sensor rotationSensor(SensorManager sensors, int reference) {
        if (reference == ARBITRARY_HEADING) {
            Sensor game = sensors.getDefaultSensor(Sensor.TYPE_GAME_ROTATION_VECTOR);
            if (game != null) {
                return game;
            }
        }
        return sensors.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR);
    }

    /** Whether the device has a rotation sensor. */
    public static boolean cranposeDeviceMotionAvailable(Context context) {
        SensorManager sensors = (SensorManager) context.getSystemService(Context.SENSOR_SERVICE);
        return sensors != null && rotationSensor(sensors, ARBITRARY_HEADING) != null;
    }

    /**
     * Starts the sensor, or switches a running one to another reference or
     * period. A period of zero is the fastest rate. Samples arrive through
     * {@code nativeOnDeviceMotion}. Returns whether a sensor now runs.
     */
    public static boolean cranposeDeviceMotionStart(
            Activity activity, int reference, int periodMicros) {
        synchronized (LOCK) {
            if (session == null) {
                SensorManager sensors =
                        (SensorManager) activity.getSystemService(Context.SENSOR_SERVICE);
                if (sensors == null) {
                    return false;
                }
                session = new CranposeDeviceMotion(activity, sensors);
                activity.getApplication().registerActivityLifecycleCallbacks(session.lifecycle);
            }
            session.release();
            Sensor sensor = rotationSensor(session.sensors, reference);
            if (sensor == null) {
                return false;
            }
            session.sensor = sensor;
            session.reference = sensor.getType() == Sensor.TYPE_GAME_ROTATION_VECTOR
                    ? ARBITRARY_HEADING
                    : MAGNETIC_NORTH;
            session.periodMicros =
                    periodMicros <= 0 ? SensorManager.SENSOR_DELAY_FASTEST : periodMicros;
            session.listen();
            return session.listening;
        }
    }

    /** Stops the sensor and releases its thread. */
    public static void cranposeDeviceMotionStop(Activity activity) {
        synchronized (LOCK) {
            if (session == null) {
                return;
            }
            session.release();
            activity.getApplication().unregisterActivityLifecycleCallbacks(session.lifecycle);
            session.thread.quitSafely();
            session = null;
        }
    }

    private void listen() {
        if (listening || sensor == null) {
            return;
        }
        // No report latency: a batched sample would reach Rust late.
        listening = sensors.registerListener(this, sensor, periodMicros, 0, handler);
    }

    private void release() {
        if (!listening) {
            return;
        }
        sensors.unregisterListener(this);
        listening = false;
    }

    @Override
    public void onSensorChanged(SensorEvent event) {
        float[] values = event.values;
        if (values.length < 3) {
            return;
        }
        nativeOnDeviceMotion(
                values[0],
                values[1],
                values[2],
                values.length > 3 ? values[3] : Float.NaN,
                event.timestamp,
                reference);
    }

    @Override
    public void onAccuracyChanged(Sensor changed, int accuracy) {}
}
