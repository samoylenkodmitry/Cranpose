package dev.cranpose.android;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.LinkAddress;
import android.net.LinkProperties;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.NetworkRequest;

import java.net.NetworkInterface;
import java.util.HashMap;
import java.util.Map;

/**
 * Networks that the application asks the system for by their transport,
 * behind {@code cranpose_services::network_request}.
 *
 * <p>An application carries this class only when its build script declares
 * {@code Use::network_request()}; from that declaration the Cranpose Gradle
 * plugin adds this source directory and the {@code CHANGE_NETWORK_STATE}
 * permission.
 *
 * <p>Each request keeps its callback until Rust releases it. The callbacks run
 * on the system's connectivity thread and report each network with the
 * properties of its link.
 */
public final class CranposeNetworkRequest {
    /** The values of {@code cranpose_services::NetworkTransport}, in its order. */
    private static final int[] TRANSPORTS = {
        NetworkCapabilities.TRANSPORT_CELLULAR,
        NetworkCapabilities.TRANSPORT_WIFI,
        NetworkCapabilities.TRANSPORT_BLUETOOTH,
        NetworkCapabilities.TRANSPORT_ETHERNET,
        NetworkCapabilities.TRANSPORT_USB,
    };

    private static native void nativeOnAvailable(
            long id, long handle, String interfaceName, int interfaceIndex,
            String addresses, int downstreamKbps, int upstreamKbps);

    private static native void nativeOnLost(long id);

    private static native void nativeOnUnavailable(long id);

    private static final Map<Long, ConnectivityManager.NetworkCallback> CALLBACKS = new HashMap<>();

    private CranposeNetworkRequest() {}

    /** Asks the system for a network; false when it took no request. */
    public static boolean cranposeRequestNetwork(
            Context context, long id, int transport, boolean internet) {
        ConnectivityManager manager =
                (ConnectivityManager) context.getSystemService(Context.CONNECTIVITY_SERVICE);
        if (manager == null || transport < 0 || transport >= TRANSPORTS.length) {
            return false;
        }
        NetworkRequest.Builder builder =
                new NetworkRequest.Builder().addTransportType(TRANSPORTS[transport]);
        if (!internet) {
            // A link to one device, such as a headset's USB network, has no
            // internet. The system does not count it as trusted either.
            builder.removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                    .removeCapability(NetworkCapabilities.NET_CAPABILITY_TRUSTED);
        }
        ConnectivityManager.NetworkCallback callback = new ConnectivityManager.NetworkCallback() {
            @Override
            public void onAvailable(Network network) {
                report(manager, id, network);
            }

            @Override
            public void onCapabilitiesChanged(Network network, NetworkCapabilities capabilities) {
                report(manager, id, network);
            }

            @Override
            public void onLinkPropertiesChanged(Network network, LinkProperties properties) {
                report(manager, id, network);
            }

            @Override
            public void onLost(Network network) {
                nativeOnLost(id);
            }

            @Override
            public void onUnavailable() {
                synchronized (CALLBACKS) {
                    CALLBACKS.remove(id);
                }
                nativeOnUnavailable(id);
            }
        };
        try {
            manager.requestNetwork(builder.build(), callback);
        } catch (RuntimeException refused) {
            return false;
        }
        synchronized (CALLBACKS) {
            CALLBACKS.put(id, callback);
        }
        return true;
    }

    /** Gives the network of the request back. */
    public static void cranposeReleaseNetwork(Context context, long id) {
        ConnectivityManager.NetworkCallback callback;
        synchronized (CALLBACKS) {
            callback = CALLBACKS.remove(id);
        }
        ConnectivityManager manager =
                (ConnectivityManager) context.getSystemService(Context.CONNECTIVITY_SERVICE);
        if (callback == null || manager == null) {
            return;
        }
        try {
            manager.unregisterNetworkCallback(callback);
        } catch (RuntimeException alreadyGone) {
            // The system dropped the request already.
        }
    }

    private static void report(ConnectivityManager manager, long id, Network network) {
        LinkProperties link = manager.getLinkProperties(network);
        NetworkCapabilities capabilities = manager.getNetworkCapabilities(network);
        // Both arrive right after onAvailable. A network without its link
        // properties has no interface to bind or to address yet.
        if (link == null || link.getInterfaceName() == null || capabilities == null) {
            return;
        }
        String name = link.getInterfaceName();
        int index = 0;
        try {
            NetworkInterface found = NetworkInterface.getByName(name);
            if (found != null) {
                index = found.getIndex();
            }
        } catch (Exception unknown) {
            // A link-local address then has no scope; the rest still works.
        }
        // Rust reads the addresses as one comma-separated text
        StringBuilder addresses = new StringBuilder();
        for (LinkAddress address : link.getLinkAddresses()) {
            if (addresses.length() > 0) {
                addresses.append(',');
            }
            addresses.append(address.getAddress().getHostAddress());
        }
        nativeOnAvailable(id, network.getNetworkHandle(), name, index, addresses.toString(),
                capabilities.getLinkDownstreamBandwidthKbps(),
                capabilities.getLinkUpstreamBandwidthKbps());
    }
}
