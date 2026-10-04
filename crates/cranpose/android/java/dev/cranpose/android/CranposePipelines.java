package dev.cranpose.android;

import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;

/**
 * Builds an updated app's pipelines before its first launch.
 *
 * <p>Android sends {@link Intent#ACTION_MY_PACKAGE_REPLACED} to an app it has
 * just updated. The driver's compiled pipelines belong to the build that
 * compiled them, so the first launch after an update would compile its first
 * screen again. The Cranpose manifest declares this receiver for every app: it
 * builds the pipelines the app's last launches drew, in the background, and
 * keeps the broadcast alive with {@link #goAsync} while it does.
 */
public final class CranposePipelines extends BroadcastReceiver {
    private static native void nativePrepareAfterUpdate(String dataDir);

    @Override
    public void onReceive(Context context, Intent intent) {
        if (!Intent.ACTION_MY_PACKAGE_REPLACED.equals(intent.getAction())) {
            return;
        }
        Context app = context.getApplicationContext();
        String dataDir = app.getFilesDir().getAbsolutePath();
        PendingResult pending = goAsync();
        new Thread(
                        () -> {
                            try {
                                CranposeActivity.loadNativeLibrary(
                                        app, new ComponentName(app, CranposeActivity.class));
                                nativePrepareAfterUpdate(dataDir);
                            } catch (UnsatisfiedLinkError error) {
                                android.util.Log.w(
                                        "cranpose", "could not prepare pipelines after an update", error);
                            } finally {
                                pending.finish();
                            }
                        },
                        "cranpose-pipeline-update")
                .start();
    }
}
