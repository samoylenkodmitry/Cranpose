package dev.cranpose.android;

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
 * with it, when a message or a stream arrives. A process the service started
 * has no activity: {@link CranposeProcess} starts the native side, where the
 * application installs its receiver in {@code AppLauncher::with_process_start}.
 */
public final class CranposeWearableService extends WearableListenerService {
    private static final String TAG = "CranposeWearable";
    private static final long TIMEOUT_SECONDS = 10;
    private boolean loaded;

    private static native void nativeOnMessage(String peer, String path, byte[] data);

    private static native void nativeOnStream(String peer, String path, InputStream stream);

    @Override
    public void onCreate() {
        super.onCreate();
        loaded = CranposeProcess.start(this);
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
}
