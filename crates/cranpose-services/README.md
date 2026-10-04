# Cranpose services

`cranpose-services` defines platform service contracts for Cranpose applications. The crate groups APIs for HTTP, files and folders, URI handlers, audio and media, camera, image pickers, haptics, heart rate, accessibility, device and power information, app updates, purchases, navigation, preferences, and shared content.

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

The heart-rate sensor is in a build only when its build script declares it, and
the person is asked only when the application asks:

```rust
// build.rs
cranpose_capabilities::declare(&[Use::heart_rate(
    "Plays to your pulse. The reading never leaves this watch.",
)])
.emit();

// a screen: the sensor runs while this is composed and `active`
let heart = rememberHeartRate(active);
if let Some(bpm) = heart.get().live_bpm() {
    game.set_pulse(bpm);
}

// when the person chooses the feature that reads it, never on the app's own
request_heart_rate_permission();
```

Observing never prompts: an application that has not asked sees
`HeartRateStatus::NeedsPermission`, and one that was refused sees `Denied`.
`live_bpm()` is a reading only while the sensor is on the skin. On Android the
service is compiled into the app only when declared, and the sensor pauses with
the activity; other platforms report `Unavailable`.

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
