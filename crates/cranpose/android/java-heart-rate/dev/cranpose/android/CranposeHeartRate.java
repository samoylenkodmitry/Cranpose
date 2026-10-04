package dev.cranpose.android;

import android.app.Activity;
import android.app.Application;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.hardware.Sensor;
import android.hardware.SensorEvent;
import android.hardware.SensorEventListener;
import android.hardware.SensorManager;
import android.os.Build;
import android.os.Bundle;

/**
 * The wearer's heart rate, behind {@code cranpose_services::heart_rate}.
 *
 * <p>An application carries this class only when its build script declares
 * {@code Use::heart_rate(...)}; from that declaration the Cranpose Gradle
 * plugin adds this source directory and the permissions, and nothing of it
 * reaches an application that does not declare it.
 *
 * <p>Nothing here asks the person on its own. The permission is requested
 * only by {@link #cranposeHeartRateRequestPermission}, which Rust calls only
 * when the application calls {@code request_heart_rate_permission()}. The
 * answer reaches {@code CranposeActivity#onRequestPermissionsResult}, which
 * hands any result it did not ask for itself to Rust.
 *
 * <p>The sensor runs only while Rust has a reader for it and the activity is
 * in front; a paused activity releases it and takes it back on resume.
 */
public final class CranposeHeartRate implements SensorEventListener {
    /** The sensor is warming up or between readings. */
    static final int ACQUIRING = 0;
    /** {@code bpm} is a reading. */
    static final int LIVE = 1;
    /** The sensor says it is not touching skin. */
    static final int OFF_BODY = 2;

    private static final String PREFERENCES = "dev.cranpose.heart_rate";
    private static final String ASKED = "asked";
    private static final int REQUEST_CODE = 0x0C9A0000 + 0x200;

    private static native void nativeOnHeartRate(float bpm, int status);

    private static CranposeHeartRate session;

    private final Activity activity;
    private final SensorManager sensors;
    private final Sensor sensor;
    private final Application.ActivityLifecycleCallbacks lifecycle;
    private boolean listening;

    private CranposeHeartRate(Activity activity, SensorManager sensors, Sensor sensor) {
        this.activity = activity;
        this.sensors = sensors;
        this.sensor = sensor;
        this.lifecycle = new Application.ActivityLifecycleCallbacks() {
            @Override
            public void onActivityResumed(Activity resumed) {
                if (resumed == CranposeHeartRate.this.activity) {
                    listen();
                }
            }

            @Override
            public void onActivityPaused(Activity paused) {
                if (paused == CranposeHeartRate.this.activity) {
                    release();
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

    /** The permission this release of Android reads the sensor under. */
    static String permissionName() {
        return Build.VERSION.SDK_INT >= 36
                ? "android.permission.health.READ_HEART_RATE"
                : "android.permission.BODY_SENSORS";
    }

    private static Sensor heartSensor(Context context) {
        SensorManager sensors = (SensorManager) context.getSystemService(Context.SENSOR_SERVICE);
        return sensors == null ? null : sensors.getDefaultSensor(Sensor.TYPE_HEART_RATE);
    }

    /** Whether the device has a heart-rate sensor. */
    public static boolean cranposeHeartRateAvailable(Context context) {
        return heartSensor(context) != null;
    }

    /** 0 not asked, 1 granted, 2 refused. */
    public static int cranposeHeartRatePermission(Activity activity) {
        if (activity.checkSelfPermission(permissionName()) == PackageManager.PERMISSION_GRANTED) {
            return 1;
        }
        SharedPreferences preferences =
                activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE);
        return preferences.getBoolean(ASKED, false) ? 2 : 0;
    }

    /** Shows the system prompt. Called only on the application's explicit request. */
    public static void cranposeHeartRateRequestPermission(Activity activity) {
        activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putBoolean(ASKED, true)
                .apply();
        activity.runOnUiThread(() ->
                activity.requestPermissions(new String[] {permissionName()}, REQUEST_CODE));
    }

    /** Starts reading; readings arrive through {@code nativeOnHeartRate}. */
    public static void cranposeHeartRateStart(Activity activity) {
        activity.runOnUiThread(() -> {
            if (session != null) {
                return;
            }
            SensorManager sensors =
                    (SensorManager) activity.getSystemService(Context.SENSOR_SERVICE);
            Sensor sensor = sensors == null ? null : sensors.getDefaultSensor(Sensor.TYPE_HEART_RATE);
            if (sensor == null) {
                return;
            }
            session = new CranposeHeartRate(activity, sensors, sensor);
            activity.getApplication().registerActivityLifecycleCallbacks(session.lifecycle);
            session.listen();
        });
    }

    /** Stops reading and releases the sensor. */
    public static void cranposeHeartRateStop(Activity activity) {
        activity.runOnUiThread(() -> {
            if (session == null) {
                return;
            }
            session.release();
            activity.getApplication().unregisterActivityLifecycleCallbacks(session.lifecycle);
            session = null;
        });
    }

    private void listen() {
        if (listening) {
            return;
        }
        listening = sensors.registerListener(this, sensor, SensorManager.SENSOR_DELAY_NORMAL);
        if (listening) {
            nativeOnHeartRate(0f, ACQUIRING);
        }
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
        if (event.sensor.getType() != Sensor.TYPE_HEART_RATE || event.values.length == 0) {
            return;
        }
        float bpm = event.values[0];
        if (event.accuracy == SensorManager.SENSOR_STATUS_NO_CONTACT) {
            nativeOnHeartRate(0f, OFF_BODY);
        } else if (bpm <= 0f || event.accuracy == SensorManager.SENSOR_STATUS_UNRELIABLE) {
            nativeOnHeartRate(0f, ACQUIRING);
        } else {
            nativeOnHeartRate(bpm, LIVE);
        }
    }

    @Override
    public void onAccuracyChanged(Sensor changed, int accuracy) {
        if (accuracy == SensorManager.SENSOR_STATUS_NO_CONTACT) {
            nativeOnHeartRate(0f, OFF_BODY);
        }
    }
}
