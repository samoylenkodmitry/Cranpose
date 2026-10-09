package dev.cranpose.android;

import android.content.Context;
import android.content.Intent;
import android.net.Uri;

import androidx.wear.remote.interactions.RemoteActivityHelper;

import com.google.android.gms.tasks.Tasks;
import com.google.android.gms.wearable.CapabilityClient;
import com.google.android.gms.wearable.CapabilityInfo;
import com.google.android.gms.wearable.ChannelClient;
import com.google.android.gms.wearable.Node;
import com.google.android.gms.wearable.Wearable;

import java.io.OutputStream;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
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

    /**
     * The capability the build advertises for this application (see the
     * Gradle plugin's wearable capability resource): the same application id
     * on the phone and the watch gives the same name.
     */
    static String capability(Context context) {
        return "cranpose_app_" + context.getPackageName().replace('.', '_');
    }

    /**
     * The connected devices, one {@code id<TAB>name<TAB>app} line each, where
     * {@code app} is 1 when this application is installed there.
     */
    static String peers(Context context) throws Exception {
        List<Node> nodes = Tasks.await(
                Wearable.getNodeClient(context).getConnectedNodes(), TIMEOUT_SECONDS, TimeUnit.SECONDS);
        CapabilityInfo installed = Tasks.await(
                Wearable.getCapabilityClient(context)
                        .getCapability(capability(context), CapabilityClient.FILTER_REACHABLE),
                TIMEOUT_SECONDS, TimeUnit.SECONDS);
        Set<String> withApp = new HashSet<>();
        for (Node node : installed.getNodes()) {
            withApp.add(node.getId());
        }
        StringBuilder listing = new StringBuilder();
        for (Node node : nodes) {
            listing.append(node.getId()).append('\t').append(node.getDisplayName()).append('\t')
                    .append(withApp.contains(node.getId()) ? '1' : '0').append('\n');
        }
        return listing.toString();
    }

    /** Opens {@code url} on {@code peer}'s screen, such as its store page for this application. */
    static void openUrl(Context context, String peer, String url) throws Exception {
        Intent view = new Intent(Intent.ACTION_VIEW)
                .addCategory(Intent.CATEGORY_BROWSABLE)
                .setData(Uri.parse(url));
        // The caller waits on the result, so the helper's callbacks run on
        // whichever thread completes them.
        new RemoteActivityHelper(context, Runnable::run)
                .startRemoteActivity(view, peer)
                .get(TIMEOUT_SECONDS, TimeUnit.SECONDS);
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
