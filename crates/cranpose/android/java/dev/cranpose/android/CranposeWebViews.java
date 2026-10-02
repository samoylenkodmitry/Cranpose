package dev.cranpose.android;

import android.app.Activity;
import android.graphics.PixelFormat;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.widget.FrameLayout;
import java.util.HashMap;

final class CranposeWebViews {
    private final Activity activity;
    private final HashMap<Long, Child> views = new HashMap<>();
    private boolean visible = true;
    private boolean closed;

    CranposeWebViews(Activity activity) { this.activity = activity; }

    void update(long id, String url, boolean placed, float x, float y, float width, float height) {
        activity.runOnUiThread(() -> {
            if (closed) return;
            Child child = views.computeIfAbsent(id, key -> new Child(key));
            if (placed) child.place(x, y, width, height);
            if (url != null) child.web.navigate(url);
        });
    }

    void remove(long id) {
        activity.runOnUiThread(() -> {
            Child child = views.remove(id);
            if (child != null) child.close();
        });
    }

    void setVisible(boolean visible) {
        this.visible = visible;
        for (Child child : views.values()) child.show();
    }

    void close() {
        closed = true;
        for (Child child : views.values()) child.close();
        views.clear();
    }

    private final class Child {
        final CranposeWebView web;
        final FrameLayout clip = new FrameLayout(activity);
        final WindowManager manager = activity.getWindowManager();
        final WindowManager.LayoutParams params = new WindowManager.LayoutParams(
            1, 1, WindowManager.LayoutParams.TYPE_APPLICATION_PANEL,
            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT);
        boolean attached;
        boolean placed;

        Child(long id) {
            web = new CranposeWebView(activity, event -> nativeEvent(id, event));
            clip.setClipChildren(true);
            clip.addView(web);
            params.token = activity.getWindow().getDecorView().getWindowToken();
            params.gravity = Gravity.TOP | Gravity.LEFT;
            params.setTitle("Cranpose WebView");
        }

        void place(float x, float y, float width, float height) {
            float density = activity.getResources().getDisplayMetrics().density;
            int left = Math.round(x * density), top = Math.round(y * density);
            int w = Math.max(0, Math.round(width * density)), h = Math.max(0, Math.round(height * density));
            View host = activity.getWindow().getDecorView();
            params.x = Math.max(0, left);
            params.y = Math.max(0, top);
            params.width = Math.max(0, Math.min(host.getWidth(), left + w) - params.x);
            params.height = Math.max(0, Math.min(host.getHeight(), top + h) - params.y);
            FrameLayout.LayoutParams content = new FrameLayout.LayoutParams(w, h);
            content.leftMargin = left - params.x;
            content.topMargin = top - params.y;
            web.setLayoutParams(content);
            placed = params.width > 0 && params.height > 0;
            show();
            if (attached) manager.updateViewLayout(clip, params);
        }

        void show() {
            boolean show = visible && placed && !closed;
            if (show && !attached) {
                manager.addView(clip, params);
                attached = true;
                web.onResume();
            } else if (!show && attached) {
                web.onPause();
                manager.removeViewImmediate(clip);
                attached = false;
            }
        }

        void close() {
            if (attached) manager.removeViewImmediate(clip);
            attached = false;
            clip.removeView(web);
            web.close();
        }
    }

    private static native void nativeEvent(long route, String event);
}
