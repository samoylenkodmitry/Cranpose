package dev.cranpose.android;

import android.app.Activity;
import android.graphics.Bitmap;
import android.os.Build;
import android.util.LruCache;
import android.view.PointerIcon;
import android.view.View;
import android.view.ViewGroup;

final class CranposePointerIcons {
    private final LruCache<Long, PointerIcon> icons = new LruCache<>(32);

    void apply(Activity activity, int type, long identity, byte[] rgba, int width,
            int height, float hotspotX, float hotspotY) {
        activity.runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT < 24) return;
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
            applyToView(activity.getWindow().getDecorView(), icon);
        });
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
