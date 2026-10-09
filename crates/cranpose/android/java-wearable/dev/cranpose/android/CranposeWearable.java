package dev.cranpose.android;

import android.content.Context;

import com.google.android.gms.tasks.Tasks;
import com.google.android.gms.wearable.ChannelClient;
import com.google.android.gms.wearable.Node;
import com.google.android.gms.wearable.Wearable;

import java.io.OutputStream;
import java.util.List;
import java.util.concurrent.TimeUnit;

/**
 * Messages and byte streams to the same application on the paired phone or
 * watch, behind {@code cranpose_services::wearable}.
 *
 * <p>An application carries this class only when its Gradle build declares
 * {@code cranpose { services.add("wearable") }}, which also brings Google Play
 * services' wearable library and {@link CranposeWearableService}, the
 * receiving end.
 *
 * <p>Every method blocks until Google Play services answers; Rust calls them
 * off the main thread.
 */
public final class CranposeWearable {
    private static final long TIMEOUT_SECONDS = 10;

    private CranposeWearable() {}

    /** The connected devices, one {@code id<TAB>name} line each. */
    static String peers(Context context) throws Exception {
        List<Node> nodes = Tasks.await(
                Wearable.getNodeClient(context).getConnectedNodes(), TIMEOUT_SECONDS, TimeUnit.SECONDS);
        StringBuilder listing = new StringBuilder();
        for (Node node : nodes) {
            listing.append(node.getId()).append('\t').append(node.getDisplayName()).append('\n');
        }
        return listing.toString();
    }

    /** Delivers {@code data} to the application on {@code peer} under {@code path}. */
    static void sendMessage(Context context, String peer, String path, byte[] data) throws Exception {
        Tasks.await(
                Wearable.getMessageClient(context).sendMessage(peer, path, data), TIMEOUT_SECONDS, TimeUnit.SECONDS);
    }

    /** Opens a byte stream to the application on {@code peer} under {@code path}. */
    static OutputStream openStream(Context context, String peer, String path) throws Exception {
        ChannelClient channels = Wearable.getChannelClient(context);
        ChannelClient.Channel channel = Tasks.await(channels.openChannel(peer, path), TIMEOUT_SECONDS, TimeUnit.SECONDS);
        return Tasks.await(channels.getOutputStream(channel), TIMEOUT_SECONDS, TimeUnit.SECONDS);
    }
}
