//! The services that an application carries only when its build script
//! declares them: each brings its Java class and its permissions, and
//! anywhere else the service reports that it has nothing.

use cranpose_capabilities::{Capabilities, Service};

pub(crate) fn register(app: &android_activity::AndroidApp, capabilities: &Capabilities<'_>) {
    if capabilities.has(Service::Update) {
        super::app_update::register(app.clone());
    }
    // Only an application that declared the sensor carries its Java, its
    // permissions and this backend; anywhere else the service reports
    // that there is no heart rate to read.
    if capabilities.has(Service::HeartRate) {
        super::heart_rate::register(app.clone());
    }
    // Present only when the application declares the microphone; anywhere
    // else recording needs no permission from the framework.
    if capabilities.has(Service::Microphone) {
        super::microphone::register(app.clone());
    }
    // Present only when the application declares the unlock, which brings
    // `CranposeDeviceAuth`; anywhere else nothing can unlock.
    if capabilities.has(Service::DeviceAuth) {
        super::device_auth::register(app.clone());
    }
    // Present only when the application declares the rotation sensor, which
    // brings `CranposeDeviceMotion`; anywhere else the service reports that
    // there is nothing to read.
    if capabilities.has(Service::DeviceMotion) {
        super::device_motion::register(app.clone());
    }
}
