# Cranpose services

`cranpose-services` defines platform service contracts for Cranpose applications. The crate groups APIs for HTTP, files and folders, URI handlers, audio and media, camera, hardware video decoding and the surfaces it writes into, image pickers, haptics, heart rate, network requests, the device's rotation, the device unlock, accessibility, device and power information, app updates, purchases, navigation, preferences, and shared content.

Most applications use these APIs through the `cranpose` facade. Add `cranpose-services` directly when an application provides a custom backend or needs a service contract outside the facade. Platform adapters register implementations through each module's `set_platform_*` function. Composition-local services use `local_*` functions and `Provide*` composables; process-wide services use plain functions.

## Example: read purchase state

Configure product IDs at startup, then read the current snapshot and react to store updates from app code:

```rust
use cranpose_services::purchases;

fn configure_store() {
    purchases::configure(&["com.example.pro"]);
}

fn pro_is_owned() -> bool {
    purchases::store_state().owns("com.example.pro")
}

fn current_store_phase() -> purchases::StorePhase {
    purchases::store_state().phase
}
```

Purchase events arrive asynchronously through `rememberPurchaseEvents` or `take_event`. Store platform setup in the adapter. See the [purchases API](https://docs.rs/cranpose-services/latest/cranpose_services/purchases/index.html).

## Example: read the wearer's heart rate

The heart-rate sensor is in a build only when its build script declares
`cranpose_capabilities::Use::heart_rate(reason)`, and the person is asked only
when the application asks:

```rust
use cranpose_services::{rememberHeartRate, request_heart_rate_permission};

// On a screen that reads the pulse: the sensor runs while this is composed
// and `active`, and observing it never prompts.
fn pulse(active: bool) -> Option<f32> {
    rememberHeartRate(active).get().live_bpm()
}

// When the person chooses the feature that reads it, never on the app's own.
fn on_choose_heart_mode() {
    request_heart_rate_permission();
}
```

An application that has not asked sees `HeartRateStatus::NeedsPermission`, and
one that was refused sees `Denied`. `live_bpm()` is a reading only while the
sensor is on the skin. On Android the service is compiled into the app only
when declared, and the sensor pauses with the activity; other platforms report
`Unavailable`.

## Example: find a computer over a USB cable

A headset that is plugged into a computer can offer a network over the
cable. The application asks the system for it by its transport, binds its
sockets to it and finds the computer by a query to every node on the link.
The service is in a build only when its build script declares
`cranpose_capabilities::Use::network_request()`.

```rust
use std::net::{Ipv6Addr, SocketAddr};

use cranpose_services::{
    NetworkRequest, NetworkTransport, RequestedNetwork, request_network,
};

// Held while the application wants the cable. Dropping it gives the network
// back.
fn ask_for_the_cable() -> RequestedNetwork {
    request_network(NetworkRequest::local(NetworkTransport::Usb))
}

// The computer answers from its link-local address on the cable.
fn find_the_computer(cable: &RequestedNetwork, port: u16) -> Option<SocketAddr> {
    let network = cable.network()?;
    let socket = network.bind_udp((Ipv6Addr::UNSPECIFIED, 0).into()).ok()?;
    socket.send_to(b"who is there", network.all_nodes(port)).ok()?;
    let mut answer = [0u8; 64];
    socket.recv_from(&mut answer).ok().map(|(_, from)| from)
}
```

On Android the request asks `ConnectivityManager.requestNetwork`. Android
serves it while the application is in front, so the application asks again
after `NetworkRequestState::Unavailable`. Other platforms hand no network to
one application and answer `Unavailable` at once.

## Example: turn a view with the device

The rotation of the device comes from its motion sensors, as a unit
quaternion with the sensor's own timestamp. The service is in a build only
when its build script declares `cranpose_capabilities::Use::device_motion()`;
on iOS the `cranpose` feature `device-motion` links Core Motion as well.

```rust
use cranpose_services::{
    MotionConfig, latest_device_motion, rememberDeviceMotion, rememberDeviceMotionSample,
};

// The sensor runs while this call is composed. The status is state that
// changes rarely. The call asks the person for nothing.
fn tracker() {
    let motion = rememberDeviceMotion(MotionConfig::default());
    // State of the newest sample, refreshed once per frame while live.
    let _sample = rememberDeviceMotionSample(motion);
}

// Every frame, on any thread: no allocation and no lock.
fn head_rotation() -> Option<[f32; 4]> {
    let sample = latest_device_motion()?;
    let q = sample.rotation;
    Some([q.x, q.y, q.z, q.w])
}
```

The frame of the quaternion, the timestamp clocks and the platform sources are
in the [`device_motion`](https://docs.rs/cranpose-services/latest/cranpose_services/device_motion/index.html)
module. Desktop systems report `DeviceMotionStatus::Unavailable`.

## Example: ask the owner to unlock

An application can keep a screen, such as a history, from a person who holds
the phone but does not own it. The system asks for a face, a fingerprint or
the screen lock, and tells the application only whether the owner unlocked.
The service is in a build only when its build script declares
`cranpose_capabilities::Use::device_auth(reason)`; on iOS the `cranpose`
feature `device-auth` links LocalAuthentication as well.

```rust
use cranpose_services::{
    DeviceAuthOutcome, authenticate_device_owner, rememberDeviceAuthAvailability,
};

// Whether the device can lock the screen at all. Reading it never prompts,
// and it is read again when the application comes back from the settings.
fn can_lock() -> bool {
    rememberDeviceAuthAvailability().can_authenticate()
}

// When the person opens the locked screen.
async fn unlock() -> bool {
    authenticate_device_owner("Open your history").await == DeviceAuthOutcome::Unlocked
}
```

`DeviceAuthAvailability::NoScreenLock` means the person has to set a screen
lock first. A call while a prompt is open shows no second prompt and gets the
open one's outcome. The prompt opens only while the application is in the
foreground. A call made during launch or as the application leaves the screen
waits. Its prompt opens when the application resumes. Android 10 and newer show `BiometricPrompt` with the screen
lock allowed, and Android 9 and older the confirm-credential screen. iOS
evaluates `LAPolicy::DeviceOwnerAuthentication`. Desktop systems, watches and
the web report `Unsupported`, and their prompts end with `Unavailable`.

## Saving state

`rememberSaveable(key, saver, initial)` restores once when its composition slot
is created. Native hosts save the latest values before leaving the foreground;
the browser host saves when its page loses foreground visibility or focus.
Changes do not require another frame to be included. Ordinary recomposition does
not read, serialize or write preferences again.

Each `rememberSaveable` key lives in the `SaveableScope` where it is composed.
`ProvideSaveableScope` gives content a child scope, for example one per
navigation entry, and a scope can keep its values only in memory. When a
saveable leaves the composition, its last value stays in memory until a save
boundary writes it, so content that comes back restores it.
`SaveableScope::forget` drops a scope's values and removes the stored ones at
the next save boundary, on the save worker.

Call `run_durable_saves(deadline)` on the composition thread for an explicit save
boundary before discarding a composition or finishing a critical operation.
`Completed` means all registered callbacks and preference writes succeeded.
`Failed` is retryable; `TimedOut` allows an already running batch to finish.
Abrupt termination before a successful save can lose pending changes. Browser
storage is synchronous, so its deadline can be reported but cannot interrupt I/O.

`DurableSaveEffect` and `register_durable_save` callbacks return
`Result<(), DurableSaveError>`. Preference-producing callbacks should use
`register_preference_save`: values for the same store are written as a batch.
Native saving uses at most one worker and does not queue obsolete batches.

`FilePreferences` coordinates all instances opening the same canonical file
within the process. Writes synchronize the replacement file; Unix also
synchronizes its directory. If a write fails, reads include pending changes and
`PreferencesStore::flush()` retries them. Other processes must not modify that
file concurrently. Custom backends implement `update_many` to batch their writes and removals.

## Features and platform adapters

The default feature set is empty. Select only the host integrations the application uses:

| Feature | Integration |
| --- | --- |
| `http-native` | Native HTTP client with platform TLS support |
| `web-http` | Browser HTTP client |
| `file-picker-native` | Native desktop file and folder picker |
| `file-picker-web` | Browser file and folder picker |
| `uri-native`, `uri-android`, `uri-web` | URI open requests for each host |
| `preferences-web` | Browser local storage |
| `system-theme`, `system-theme-web` | Native or browser theme detection |
| `notifier-native` | Desktop notifications through host commands |

Android and iOS file pickers register through the `cranpose` platform backends. The `cranpose` facade selects service features for its platform; app authors can also choose features directly for custom hosts. `cranpose-audio` implements short sound playback, `cranpose-media` implements native long-form playback, and `cranpose-capabilities` writes app permission declarations.

## Links

- [API documentation](https://docs.rs/cranpose-services/latest/cranpose_services/)
- [Source and examples](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-services)
- [Cranpose guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md)
