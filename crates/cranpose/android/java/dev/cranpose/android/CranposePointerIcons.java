package dev.cranpose.android;

import android.app.Activity;
import android.graphics.Bitmap;
import android.os.Build;
import android.util.LruCache;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.PointerIcon;
import android.view.View;
import android.view.ViewGroup;

final class CranposePointerIcons {
    private final LruCache<Long, PointerIcon> icons = new LruCache<>(32);
    private View pointerView;

    void apply(Activity activity, int type, long identity, byte[] rgba, int width,
            int height, float hotspotX, float hotspotY) {
        activity.runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT < 24 || activity.isDestroyed()) return;
            PointerIcon icon;
            if (rgba == null) {
                icon = PointerIcon.getSystemIcon(activity, type);
            } else {
                icon = icons.get(identity);
                if (icon == null) {
                    if (width <= 0 || height <= 0 || width > 128 || height > 128
                            || rgba.length != width * height * 4
                            || !Float.isFinite(hotspotX) || !Float.isFinite(hotspotY)
                            || hotspotX < 0 || hotspotY < 0
                            || hotspotX >= width || hotspotY >= height) return;
                    int[] argb = new int[width * height];
                    for (int i = 0; i < argb.length; i++) {
                        int at = i * 4;
                        argb[i] = ((rgba[at + 3] & 255) << 24) | ((rgba[at] & 255) << 16)
                                | ((rgba[at + 1] & 255) << 8) | (rgba[at + 2] & 255);
                    }
                    Bitmap bitmap = Bitmap.createBitmap(argb, width, height, Bitmap.Config.ARGB_8888);
                    bitmap.setDensity(Bitmap.DENSITY_NONE);
                    icon = PointerIcon.create(bitmap, hotspotX, hotspotY);
                    icons.put(identity, icon);
                }
            }
            if (pointerView == null) {
                pointerView = new PointerView(activity);
                ViewGroup content = activity.findViewById(android.R.id.content);
                // Keep native web views and editors above the pointer resolver.
                content.addView(pointerView, Math.min(1, content.getChildCount()),
                        new ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                                ViewGroup.LayoutParams.MATCH_PARENT));
            }
            applyToView(activity.getWindow().getDecorView(), icon);
        });
    }

    private static final class PointerView extends View {
        PointerView(Activity activity) {
            super(activity);
            setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
            setWillNotDraw(true);
        }

        @Override public PointerIcon onResolvePointerIcon(MotionEvent event, int index) {
            int tool = event.getToolType(index);
            if (event.isFromSource(InputDevice.SOURCE_STYLUS)
                    && (tool == MotionEvent.TOOL_TYPE_STYLUS || tool == MotionEvent.TOOL_TYPE_ERASER)) {
                return getPointerIcon();
            }
            return super.onResolvePointerIcon(event, index);
        }
    }

    private static void applyToView(View view, PointerIcon icon) {
        view.setPointerIcon(icon);
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                applyToView(group.getChildAt(i), icon);
            }
        }
    }
}
