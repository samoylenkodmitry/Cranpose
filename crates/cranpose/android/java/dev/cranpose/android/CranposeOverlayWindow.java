package dev.cranpose.android;

import android.Manifest;
import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.graphics.PixelFormat;
import android.os.Build;
import android.provider.Settings;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.WindowManager;

public final class CranposeOverlayWindow {
    public static final int RESULT_OK = 0;
    public static final int RESULT_UNSUPPORTED_SDK = -1;
    public static final int RESULT_MISSING_MANIFEST_PERMISSION = -2;
    public static final int RESULT_MISSING_RUNTIME_PERMISSION = -3;
    public static final int RESULT_ALREADY_VISIBLE = -4;
    public static final int RESULT_CREATE_FAILED = -5;
    public static final int RESULT_NOT_VISIBLE = -6;

    private static volatile SurfaceView surfaceView;

    private CranposeOverlayWindow() {
    }

    public static int show(
            Activity activity,
            int widthPx,
            int heightPx,
            int xPx,
            int yPx,
            boolean focusable
    ) {
        if (surfaceView != null) {
            return RESULT_ALREADY_VISIBLE;
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return RESULT_UNSUPPORTED_SDK;
        }
        if (!declaresOverlayPermission(activity)) {
            return RESULT_MISSING_MANIFEST_PERMISSION;
        }
        if (!Settings.canDrawOverlays(activity)) {
            return RESULT_MISSING_RUNTIME_PERMISSION;
        }

        activity.runOnUiThread(() -> {
            if (surfaceView != null) {
                return;
            }

            WindowManager windowManager =
                    (WindowManager) activity.getSystemService(Context.WINDOW_SERVICE);
            if (windowManager == null) {
                nativeOverlayCreateFailed("Android WindowManager is unavailable");
                return;
            }

            SurfaceView view = new SurfaceView(activity);
            view.setZOrderOnTop(true);
            view.getHolder().setFormat(PixelFormat.TRANSLUCENT);
            view.getHolder().addCallback(new SurfaceHolder.Callback() {
                @Override
                public void surfaceCreated(SurfaceHolder holder) {
                    Surface surface = holder.getSurface();
                    nativeOverlaySurfaceChanged(
                            surface,
                            Math.max(view.getWidth(), 1),
                            Math.max(view.getHeight(), 1)
                    );
                }

                @Override
                public void surfaceChanged(
                        SurfaceHolder holder,
                        int format,
                        int width,
                        int height
                ) {
                    nativeOverlaySurfaceChanged(holder.getSurface(), width, height);
                }

                @Override
                public void surfaceDestroyed(SurfaceHolder holder) {
                    nativeOverlaySurfaceDestroyed();
                }
            });
            view.setOnTouchListener((View touched, MotionEvent event) -> {
                nativeOverlayPointer(event.getActionMasked(), event.getX(), event.getY());
                return true;
            });

            int flags = WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS;
            if (!focusable) {
                flags |= WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE;
            }

            WindowManager.LayoutParams params = new WindowManager.LayoutParams(
                    widthPx,
                    heightPx,
                    WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
                    flags,
                    PixelFormat.TRANSLUCENT
            );
            params.gravity = Gravity.TOP | Gravity.START;
            params.x = xPx;
            params.y = yPx;

            try {
                windowManager.addView(view, params);
                surfaceView = view;
            } catch (RuntimeException error) {
                nativeOverlayCreateFailed(error.toString());
            }
        });

        return RESULT_OK;
    }

    public static int updateBounds(
            Activity activity,
            int widthPx,
            int heightPx,
            int xPx,
            int yPx
    ) {
        if (surfaceView == null) {
            return RESULT_NOT_VISIBLE;
        }

        activity.runOnUiThread(() -> {
            SurfaceView view = surfaceView;
            if (view == null) {
                return;
            }

            WindowManager windowManager =
                    (WindowManager) activity.getSystemService(Context.WINDOW_SERVICE);
            if (windowManager == null) {
                nativeOverlayCreateFailed("Android WindowManager is unavailable");
                return;
            }

            try {
                WindowManager.LayoutParams params =
                        (WindowManager.LayoutParams) view.getLayoutParams();
                params.width = widthPx;
                params.height = heightPx;
                params.x = xPx;
                params.y = yPx;
                windowManager.updateViewLayout(view, params);
            } catch (RuntimeException error) {
                nativeOverlayCreateFailed(error.toString());
            }
        });

        return RESULT_OK;
    }

    public static void hide(Activity activity) {
        activity.runOnUiThread(() -> {
            SurfaceView view = surfaceView;
            if (view == null) {
                return;
            }
            surfaceView = null;
            WindowManager windowManager =
                    (WindowManager) activity.getSystemService(Context.WINDOW_SERVICE);
            if (windowManager != null) {
                windowManager.removeViewImmediate(view);
            }
            nativeOverlaySurfaceDestroyed();
        });
    }

    private static boolean declaresOverlayPermission(Activity activity) {
        try {
            PackageInfo info = activity
                    .getPackageManager()
                    .getPackageInfo(activity.getPackageName(), PackageManager.GET_PERMISSIONS);
            if (info.requestedPermissions == null) {
                return false;
            }
            for (String permission : info.requestedPermissions) {
                if (Manifest.permission.SYSTEM_ALERT_WINDOW.equals(permission)) {
                    return true;
                }
            }
        } catch (PackageManager.NameNotFoundException ignored) {
        }
        return false;
    }

    private static native void nativeOverlayCreateFailed(String message);

    private static native void nativeOverlaySurfaceChanged(Surface surface, int width, int height);

    private static native void nativeOverlaySurfaceDestroyed();

    private static native void nativeOverlayPointer(int action, float x, float y);
}
