package dev.cranpose.android;

import android.content.ComponentName;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.util.Log;

import com.google.android.gms.tasks.Tasks;
import com.google.android.gms.wearable.ChannelClient;
import com.google.android.gms.wearable.MessageEvent;
import com.google.android.gms.wearable.Wearable;
import com.google.android.gms.wearable.WearableListenerService;

import java.io.InputStream;
import java.util.concurrent.TimeUnit;

/**
 * Hands what the paired device sends to Rust, which delivers it to the
 * application's {@code cranpose_services::WearableReceiver}.
 *
 * <p>Google Play services starts this service, and the application's process
 * with it, when a message or a stream arrives; the service loads the
 * application's native library if the activity has not. A process the
 * service started has no activity, and Rust drops what arrives until the
 * application installs its receiver.
 */
public final class CranposeWearableService extends WearableListenerService {
    private static final String TAG = "CranposeWearable";
    private static final long TIMEOUT_SECONDS = 10;
    private static boolean loaded;

    private static native void nativeOnMessage(String peer, String path, byte[] data);

    private static native void nativeOnStream(String peer, String path, InputStream stream);

    @Override
    public void onCreate() {
        super.onCreate();
        loaded = loaded || loadApplicationLibrary();
    }

    @Override
    public void onMessageReceived(MessageEvent event) {
        if (loaded) {
            nativeOnMessage(event.getSourceNodeId(), event.getPath(), event.getData());
        }
    }

    @Override
    public void onChannelOpened(ChannelClient.Channel channel) {
        if (!loaded) {
            return;
        }
        try {
            InputStream input = Tasks.await(
                    Wearable.getChannelClient(this).getInputStream(channel), TIMEOUT_SECONDS, TimeUnit.SECONDS);
            nativeOnStream(channel.getNodeId(), channel.getPath(), input);
        } catch (Exception error) {
            Log.w(TAG, "a stream from the paired device did not open", error);
        }
    }

    private boolean loadApplicationLibrary() {
        try {
            ActivityInfo activity = getPackageManager().getActivityInfo(
                    new ComponentName(this, CranposeActivity.class), PackageManager.GET_META_DATA);
            String library = activity.metaData == null ? null : activity.metaData.getString("android.app.lib_name");
            if (library == null) {
                return false;
            }
            System.loadLibrary(library);
            return true;
        } catch (PackageManager.NameNotFoundException | UnsatisfiedLinkError error) {
            Log.w(TAG, "the application's native library did not load", error);
            return false;
        }
    }
}
