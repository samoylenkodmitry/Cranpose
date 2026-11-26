#![expect(
    unsafe_code,
    reason = "JNI entry points and the NDK call that binds a socket to a network"
)]

//! Network requests on Android, through `CranposeNetworkRequest`, which an
//! application carries only when its build script declares
//! `Use::network_request`. The system's answers reach Rust on its
//! connectivity thread.

use std::{
    io,
    net::IpAddr,
    os::fd::{AsRawFd, BorrowedFd},
    sync::Arc,
};

use cranpose_services::{
    LinkSpeed, Network, NetworkHandle, NetworkReach, NetworkRequest, NetworkRequestId,
    NetworkRequestState, NetworkRequester, NetworkTransport, publish_network_request_state,
    set_platform_network_requester,
};
use jni::{
    EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JString, JValue},
    sys::{jint, jlong},
};

use super::jni::call_cranpose_class;

const NETWORK_REQUEST_CLASS: &str = "dev/cranpose/android/CranposeNetworkRequest";

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_network_requester(Arc::new(AndroidNetworkRequester { app }));
}

struct AndroidNetworkRequester {
    app: android_activity::AndroidApp,
}

// The index of a transport in `CranposeNetworkRequest.TRANSPORTS`.
fn transport_code(transport: NetworkTransport) -> jint {
    match transport {
        NetworkTransport::Cellular => 0,
        NetworkTransport::Wifi => 1,
        NetworkTransport::Bluetooth => 2,
        NetworkTransport::Ethernet => 3,
        NetworkTransport::Usb => 4,
    }
}

impl NetworkRequester for AndroidNetworkRequester {
    fn request(&self, id: NetworkRequestId, request: NetworkRequest) -> Result<(), String> {
        let taken =
            call_cranpose_class(&self.app, NETWORK_REQUEST_CLASS, |env, activity, class| {
                env.call_static_method(
                    class,
                    jni_str!("cranposeRequestNetwork"),
                    jni_sig!("(Landroid/content/Context;JIZ)Z"),
                    &[
                        JValue::Object(activity),
                        JValue::Long(id.get().cast_signed()),
                        JValue::Int(transport_code(request.transport)),
                        JValue::Bool(request.reach == NetworkReach::Internet),
                    ],
                )
                .and_then(jni::JValueOwned::z)
            })?;
        if taken {
            Ok(())
        } else {
            Err("the system took no request".to_owned())
        }
    }

    fn release(&self, id: NetworkRequestId) {
        let released =
            call_cranpose_class(&self.app, NETWORK_REQUEST_CLASS, |env, activity, class| {
                env.call_static_method(
                    class,
                    jni_str!("cranposeReleaseNetwork"),
                    jni_sig!("(Landroid/content/Context;J)V"),
                    &[
                        JValue::Object(activity),
                        JValue::Long(id.get().cast_signed()),
                    ],
                )
                .map(|_| ())
            });
        if let Err(error) = released {
            log::warn!("cranpose: a network request was not released: {error}");
        }
    }

    fn bind_socket(&self, network: NetworkHandle, socket: BorrowedFd<'_>) -> io::Result<()> {
        // SAFETY: `socket` is an open descriptor for the length of the call,
        // and the handle is one that `Network.getNetworkHandle` gave.
        let result = unsafe { ndk_sys::android_setsocknetwork(network.0, socket.as_raw_fd()) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

// An address as Java's `InetAddress.getHostAddress` writes it. Its scope,
// `%usb0`, is dropped: the network's interface index stands for it.
fn address(text: &str) -> Option<IpAddr> {
    text.split('%').next()?.parse().ok()
}

fn kbps(value: jint) -> Option<u32> {
    u32::try_from(value).ok().filter(|&kbps| kbps > 0)
}

fn request_id(id: jlong) -> NetworkRequestId {
    NetworkRequestId::new(id.cast_unsigned())
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeNetworkRequest_nativeOnAvailable<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    id: jlong,
    handle: jlong,
    interface_name: JString<'local>,
    interface_index: jint,
    addresses: JString<'local>,
    downstream_kbps: jint,
    upstream_kbps: jint,
) {
    let read = env.with_env(|env| -> jni::errors::Result<(String, String)> {
        Ok((
            interface_name.try_to_string(env)?,
            addresses.try_to_string(env)?,
        ))
    });
    let Outcome::Ok((interface_name, addresses)) = read.into_outcome() else {
        return;
    };
    let network = Network {
        handle: NetworkHandle(handle.cast_unsigned()),
        interface_name,
        interface_index: u32::try_from(interface_index).unwrap_or(0),
        addresses: addresses.split(',').filter_map(address).collect(),
        link_speed: LinkSpeed {
            downstream_kbps: kbps(downstream_kbps),
            upstream_kbps: kbps(upstream_kbps),
        },
    };
    publish_network_request_state(request_id(id), NetworkRequestState::Available(network));
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeNetworkRequest_nativeOnLost(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    publish_network_request_state(request_id(id), NetworkRequestState::Lost);
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeNetworkRequest_nativeOnUnavailable(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    id: jlong,
) {
    publish_network_request_state(request_id(id), NetworkRequestState::Unavailable);
}
