use cranpose_core::LaunchedEffect;
use cranpose_services::{
    authenticate_device_owner, rememberDeviceAuthAvailability, DeviceAuthAvailability,
    DeviceAuthOutcome,
};
use cranpose_ui::{composable, Button, ButtonSpec, Color, Modifier, Spacer, Text, TextStyle};

// The background work examples, then the unlock and the device's rotation.
#[composable]
pub(in crate::app) fn WorkAndUnlock() {
    super::FetchAsyncValue();
    Spacer(Modifier::empty().size_points(0.0, 20.0));
    DeviceUnlock();
    Spacer(Modifier::empty().size_points(0.0, 20.0));
    super::device_motion::DeviceMotionDemo();
}

// A line that says what the device can ask with and how the last prompt
// ended, over the button that shows the next prompt.
#[composable]
fn DeviceUnlock() {
    let availability = rememberDeviceAuthAvailability();
    let message = cranpose_core::rememberMutableStateOf(String::new);
    let request = cranpose_core::rememberMutableStateOf(|| 0u64);
    let key = request.get();
    LaunchedEffect(key, async move |_| {
        if key == 0 {
            return;
        }
        let outcome = authenticate_device_owner("Unlock the demo").await;
        message.set(match outcome {
            DeviceAuthOutcome::Unlocked => "Unlocked by the owner".to_string(),
            DeviceAuthOutcome::Cancelled => "The prompt was closed".to_string(),
            DeviceAuthOutcome::Failed => "The unlock failed".to_string(),
            DeviceAuthOutcome::Unavailable => "Nothing to unlock with".to_string(),
        });
    });
    let can = match availability {
        DeviceAuthAvailability::Unsupported => "This build cannot ask the owner to unlock",
        DeviceAuthAvailability::NoScreenLock => "Set a screen lock to try the unlock",
        DeviceAuthAvailability::Credential => "The device asks for its screen lock",
        DeviceAuthAvailability::Biometric => "The device asks for a face or a fingerprint",
    };
    let line = if message.get().is_empty() {
        can.to_string()
    } else {
        format!("{can}. {}", message.get())
    };
    Text(
        line,
        Modifier::empty()
            .background(Color(0.1, 0.18, 0.32, 0.6))
            .rounded_corners(14.0)
            .padding(10.0),
        TextStyle::default(),
    );
    Spacer(Modifier::empty().size_points(0.0, 12.0));
    Button(
        Modifier::empty()
            .rounded_corners(16.0)
            .background(Color(0.15, 0.35, 0.85, 1.0))
            .padding(12.0),
        ButtonSpec::default(),
        move || request.update(|value| *value += 1),
        || {
            Text(
                "Unlock as the owner",
                Modifier::empty().padding(6.0),
                TextStyle::default(),
            );
        },
    );
}
