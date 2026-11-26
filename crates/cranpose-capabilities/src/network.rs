//! The services that use the network.

use crate::Service;

// What a network service asks of an Android device.
pub(crate) struct NetworkUse {
    // The name in the build's own files
    pub(crate) name: &'static str,
    // Every permission the manifest declares
    pub(crate) permissions: &'static [&'static str],
}

const STATUS: NetworkUse = NetworkUse {
    name: "network",
    permissions: &[
        "android.permission.INTERNET",
        "android.permission.ACCESS_NETWORK_STATE",
    ],
};

// `requestNetwork` needs CHANGE_NETWORK_STATE, a normal permission that the
// system grants at install.
const REQUEST: NetworkUse = NetworkUse {
    name: "network-request",
    permissions: &[
        "android.permission.INTERNET",
        "android.permission.ACCESS_NETWORK_STATE",
        "android.permission.CHANGE_NETWORK_STATE",
    ],
};

pub(crate) const fn of(service: Service) -> &'static NetworkUse {
    match service {
        Service::NetworkRequest => &REQUEST,
        _ => &STATUS,
    }
}
