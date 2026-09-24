//! What an application asks of the device it runs on, written once in Rust.
//!
//! An application declares this in its build script:
//!
//! ```no_run
//! use cranpose_capabilities::{Use, declare};
//!
//! declare(&[
//!     Use::camera("Reads a receipt with the camera. Nothing leaves this device."),
//!     Use::notifications(),
//! ])
//! .emit();
//! ```
//!
//! From that one list the build writes the Android permissions and feature
//! declarations, the Apple usage descriptions, and a constant the application
//! itself reads at run time. A service is a function, so a name cannot be
//! misspelled, and a service Apple wants a sentence for takes that sentence as
//! an argument, so it cannot be forgotten.

use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

/// Something an application uses that the device has to allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Service {
    /// The camera.
    Camera,
    /// Reading the photo library.
    PhotoLibrary,
    /// Writing to the photo library.
    PhotoLibraryAdd,
    /// The microphone.
    Microphone,
    /// Where the device is.
    Location,
    /// Notifications the person sees.
    Notifications,
    /// Work that carries on with the application off screen.
    Background,
    /// Media playback that carries on with the application off screen.
    Media,
    /// Purchases through the platform store.
    Billing,
    /// A window drawn above other applications.
    Overlay,
    /// The vibrator.
    Haptics,
    /// Handing a downloaded package to the system installer.
    Update,
    /// The network, and whether the device is on one.
    Network,
}

impl Service {
    /// The name this service carries in the build's own files.
    pub const fn name(self) -> &'static str {
        match self {
            Service::Camera => "camera",
            Service::PhotoLibrary => "photo-library",
            Service::PhotoLibraryAdd => "photo-library-add",
            Service::Microphone => "microphone",
            Service::Location => "location",
            Service::Notifications => "notifications",
            Service::Background => "background",
            Service::Media => "media",
            Service::Billing => "billing",
            Service::Overlay => "overlay",
            Service::Haptics => "haptics",
            Service::Update => "update",
            Service::Network => "network",
        }
    }

    /// The Android permissions this service needs.
    ///
    /// Both photo library services are empty here: Android reads and writes
    /// photos through the system picker, which asks the person for one file
    /// and needs no permission from the application.
    pub const fn android_permissions(self) -> &'static [&'static str] {
        match self {
            Service::Camera => &["android.permission.CAMERA"],
            Service::PhotoLibrary => &[],
            Service::PhotoLibraryAdd => &[],
            Service::Microphone => &["android.permission.RECORD_AUDIO"],
            Service::Location => &["android.permission.ACCESS_COARSE_LOCATION"],
            Service::Notifications => &["android.permission.POST_NOTIFICATIONS"],
            Service::Background => &[
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_DATA_SYNC",
            ],
            Service::Media => &[
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK",
            ],
            Service::Billing => &["com.android.vending.BILLING"],
            Service::Overlay => &["android.permission.SYSTEM_ALERT_WINDOW"],
            Service::Haptics => &["android.permission.VIBRATE"],
            Service::Update => &["android.permission.REQUEST_INSTALL_PACKAGES"],
            Service::Network => &[
                "android.permission.INTERNET",
                "android.permission.ACCESS_NETWORK_STATE",
            ],
        }
    }

    /// The key an Apple platform reads the sentence from.
    pub const fn apple_key(self) -> Option<&'static str> {
        match self {
            Service::Camera => Some("NSCameraUsageDescription"),
            Service::PhotoLibrary => Some("NSPhotoLibraryUsageDescription"),
            Service::PhotoLibraryAdd => Some("NSPhotoLibraryAddUsageDescription"),
            Service::Microphone => Some("NSMicrophoneUsageDescription"),
            Service::Location => Some("NSLocationWhenInUseUsageDescription"),
            _ => None,
        }
    }

    /// Whether this service takes a sentence to show the person.
    pub const fn takes_reason(self) -> bool {
        self.apple_key().is_some()
    }
}

/// One service an application uses, with the sentence it shows if it needs one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Use {
    service: Service,
    reason: Option<&'static str>,
}

impl Use {
    /// The camera, with what the person is told before it opens.
    pub const fn camera(reason: &'static str) -> Self {
        Self {
            service: Service::Camera,
            reason: Some(reason),
        }
    }

    /// Reading the photo library, with what the person is told.
    pub const fn photo_library(reason: &'static str) -> Self {
        Self {
            service: Service::PhotoLibrary,
            reason: Some(reason),
        }
    }

    /// Writing to the photo library, with what the person is told.
    pub const fn photo_library_add(reason: &'static str) -> Self {
        Self {
            service: Service::PhotoLibraryAdd,
            reason: Some(reason),
        }
    }

    /// The microphone, with what the person is told.
    pub const fn microphone(reason: &'static str) -> Self {
        Self {
            service: Service::Microphone,
            reason: Some(reason),
        }
    }

    /// Where the device is, with what the person is told.
    pub const fn location(reason: &'static str) -> Self {
        Self {
            service: Service::Location,
            reason: Some(reason),
        }
    }

    /// Notifications the person sees.
    pub const fn notifications() -> Self {
        Self {
            service: Service::Notifications,
            reason: None,
        }
    }

    /// Work that carries on with the application off screen.
    pub const fn background() -> Self {
        Self {
            service: Service::Background,
            reason: None,
        }
    }

    /// Media playback that carries on with the application off screen.
    pub const fn media() -> Self {
        Self {
            service: Service::Media,
            reason: None,
        }
    }

    /// Purchases through the platform store.
    pub const fn billing() -> Self {
        Self {
            service: Service::Billing,
            reason: None,
        }
    }

    /// A window drawn above other applications.
    pub const fn overlay() -> Self {
        Self {
            service: Service::Overlay,
            reason: None,
        }
    }

    /// The vibrator.
    pub const fn haptics() -> Self {
        Self {
            service: Service::Haptics,
            reason: None,
        }
    }

    /// Handing a downloaded package to the system installer.
    pub const fn update() -> Self {
        Self {
            service: Service::Update,
            reason: None,
        }
    }

    /// The network, and whether the device is on one.
    pub const fn network() -> Self {
        Self {
            service: Service::Network,
            reason: None,
        }
    }

    /// Which service this is.
    pub const fn service(self) -> Service {
        self.service
    }

    /// The sentence the person is shown, for the services that have one.
    pub const fn reason(self) -> Option<&'static str> {
        self.reason
    }
}

/// Hardware an application cannot run without.
///
/// Every other feature stays optional, so the application reaches devices
/// that lack the hardware and asks the framework at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Demand {
    /// A camera.
    Camera,
    /// A microphone.
    Microphone,
    /// A watch.
    Watch,
    /// Telephony.
    Telephony,
    /// Bluetooth.
    Bluetooth,
    /// Near-field communication.
    Nfc,
    /// Wi-Fi.
    Wifi,
    /// Location hardware.
    Location,
}

impl Demand {
    /// The Android feature name this demand becomes.
    pub const fn android_feature(self) -> &'static str {
        match self {
            Demand::Camera => "android.hardware.camera",
            Demand::Microphone => "android.hardware.microphone",
            Demand::Watch => "android.hardware.type.watch",
            Demand::Telephony => "android.hardware.telephony",
            Demand::Bluetooth => "android.hardware.bluetooth",
            Demand::Nfc => "android.hardware.nfc",
            Demand::Wifi => "android.hardware.wifi",
            Demand::Location => "android.hardware.location",
        }
    }

    /// The name this demand carries in the build's own files.
    pub const fn name(self) -> &'static str {
        self.android_feature()
    }
}

/// Everything one application asks of a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities<'a> {
    /// The services the application uses.
    pub uses: &'a [Use],
    /// The hardware the application cannot run without.
    pub demands: &'a [Demand],
}

impl Capabilities<'_> {
    /// An application that asks for nothing.
    pub const NONE: Capabilities<'static> = Capabilities {
        uses: &[],
        demands: &[],
    };

    /// Whether the application declared this service.
    pub fn has(&self, service: Service) -> bool {
        self.uses.iter().any(|entry| entry.service == service)
    }

    /// The sentence declared for a service, if it has one.
    pub fn reason_for(&self, service: Service) -> Option<&'static str> {
        self.uses
            .iter()
            .find(|entry| entry.service == service)
            .and_then(|entry| entry.reason())
    }
}

/// What a build script declares, before [`Declaration::emit`] writes it out.
#[must_use = "a declaration reaches the platform builds only through emit()"]
#[derive(Clone, Copy, Debug)]
pub struct Declaration<'a> {
    capabilities: Capabilities<'a>,
}

/// Starts a declaration with the services an application uses.
pub const fn declare(uses: &[Use]) -> Declaration<'_> {
    Declaration {
        capabilities: Capabilities { uses, demands: &[] },
    }
}

impl<'a> Declaration<'a> {
    /// Adds the hardware the application cannot run without.
    pub const fn demanding(self, demands: &'a [Demand]) -> Declaration<'a> {
        Declaration {
            capabilities: Capabilities {
                uses: self.capabilities.uses,
                demands,
            },
        }
    }

    /// Writes the declaration where the application and every platform build
    /// reads it.
    ///
    /// Call this from a build script. It panics when the files cannot be
    /// written, which is what a build script does with a failure.
    pub fn emit(self) {
        let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set for a build script"));
        let package = env::var("CARGO_PKG_NAME").expect("CARGO_PKG_NAME is set for a build script");
        write_file(
            &out.join("cranpose_capabilities.rs"),
            &rust_source(&self.capabilities),
        );

        let crate_dir = PathBuf::from(
            env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set for a build script"),
        );
        let shared = shared_dir(&crate_dir, env::var_os(CAPABILITIES_DIR));
        fs::create_dir_all(&shared).expect("the shared capabilities directory");
        let outputs = shared_outputs(&shared, &package);
        let contents = [
            json(&self.capabilities),
            android_manifest(&self.capabilities),
            apple_usage(&self.capabilities),
        ];
        for (path, text) in outputs.iter().zip(contents.iter()) {
            write_file(path, text);
        }
        println!("cargo::rerun-if-changed=build.rs");
        println!("cargo::rerun-if-env-changed={CAPABILITIES_DIR}");
        for directive in rerun_directives(&outputs) {
            println!("{directive}");
        }
    }
}

/// The files [`Declaration::emit`] writes outside `OUT_DIR`.
///
/// They go where every platform build reads them, which is also where anything
/// that reclaims build artifacts can remove them.
fn shared_outputs(shared: &Path, package: &str) -> [PathBuf; 3] {
    [
        shared.join(format!("{package}-capabilities.json")),
        shared.join(format!("{package}-permissions.xml")),
        shared.join(format!("{package}-usage.plist")),
    ]
}

/// Tells cargo that these files are this build script's outputs.
///
/// Cargo does not know what a build script writes outside `OUT_DIR`, and a
/// path named to `rerun-if-changed` counts as changed when it is missing. So
/// naming them is what makes a deleted declaration come back: without it,
/// cargo reads an unchanged `build.rs`, skips the script, and the tree keeps
/// building without the permissions XML that carries SYSTEM_ALERT_WINDOW and
/// VIBRATE into the merged Android manifest. The Android release APK then
/// fails `cranposeReleaseManifestCheck` on every run in that workspace,
/// because nothing will ever write the file again.
fn rerun_directives(outputs: &[PathBuf]) -> Vec<String> {
    outputs
        .iter()
        .map(|path| format!("cargo::rerun-if-changed={}", path.display()))
        .collect()
}

fn write_file(path: &Path, text: &str) {
    fs::write(path, text).unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
}

/// Names the directory the declaration is written into.
///
/// A platform build sets it, because the build knows the tree it drives. A
/// plain `cargo build` does not, and the declaration then goes to
/// `<workspace>/target/cranpose`.
const CAPABILITIES_DIR: &str = "CRANPOSE_CAPABILITIES_DIR";

/// Where the declaration goes: the directory the build named, or the one under
/// the workspace this crate belongs to.
fn shared_dir(crate_dir: &Path, named: Option<OsString>) -> PathBuf {
    match named.filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => workspace_root(crate_dir).join("target").join("cranpose"),
    }
}

/// The workspace this crate belongs to, found from its own directory.
///
/// The platform builds read the declaration from `<workspace>/target/cranpose`,
/// and they know the workspace because they already resolved this crate's
/// source there. The directory Cargo happens to build into is not that place:
/// it moves with `CARGO_TARGET_DIR`, and continuous integration sets it.
fn workspace_root(crate_dir: &Path) -> PathBuf {
    found_workspace(crate_dir, &|path| {
        fs::read_to_string(path.join("Cargo.toml"))
            .is_ok_and(|manifest| manifest.contains("[workspace]"))
    })
}

fn found_workspace(crate_dir: &Path, holds_workspace: &dyn Fn(&Path) -> bool) -> PathBuf {
    // The outermost workspace, so a crate inside a workspace that is itself
    // vendored into another one still answers with the tree the build drives.
    crate_dir
        .ancestors()
        .filter(|path| holds_workspace(path))
        .last()
        .map_or_else(|| crate_dir.to_path_buf(), Path::to_path_buf)
}

/// The Rust the application includes, so it reads the same declaration the
/// platform builds do.
fn rust_source(capabilities: &Capabilities<'_>) -> String {
    let mut text = String::from(
        "pub const CAPABILITIES: cranpose_capabilities::Capabilities =\n    \
         cranpose_capabilities::Capabilities {\n        uses: &[\n",
    );
    for entry in capabilities.uses {
        let call = constructor(entry);
        let _ = writeln!(text, "            cranpose_capabilities::Use::{call},");
    }
    text.push_str("        ],\n        demands: &[\n");
    for demand in capabilities.demands {
        let _ = writeln!(
            text,
            "            cranpose_capabilities::Demand::{demand:?},"
        );
    }
    text.push_str("        ],\n    };\n");
    text
}

/// The call that rebuilds one entry, which is the service's own name with the
/// dashes a Rust function cannot carry turned back into underscores.
fn constructor(entry: &Use) -> String {
    let name = entry.service.name().replace('-', "_");
    match entry.reason {
        Some(reason) => format!("{name}(\"{}\")", escape(reason)),
        None => format!("{name}()"),
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The declaration as the platform builds read it.
pub fn json(capabilities: &Capabilities<'_>) -> String {
    let mut text = String::from("{\n  \"services\": [\n");
    for (at, entry) in capabilities.uses.iter().enumerate() {
        let comma = if at + 1 == capabilities.uses.len() {
            ""
        } else {
            ","
        };
        let reason = match entry.reason {
            Some(reason) => format!("\"{}\"", escape_json(reason)),
            None => String::from("null"),
        };
        let _ = writeln!(
            text,
            "    {{ \"name\": \"{}\", \"reason\": {reason} }}{comma}",
            entry.service.name()
        );
    }
    text.push_str("  ],\n  \"permissions\": [\n");
    let permissions = android_permissions(capabilities);
    for (at, permission) in permissions.iter().enumerate() {
        let comma = if at + 1 == permissions.len() { "" } else { "," };
        let _ = writeln!(text, "    \"{permission}\"{comma}");
    }
    text.push_str("  ],\n  \"demands\": [\n");
    for (at, demand) in capabilities.demands.iter().enumerate() {
        let comma = if at + 1 == capabilities.demands.len() {
            ""
        } else {
            ","
        };
        let _ = writeln!(text, "    \"{}\"{comma}", demand.android_feature());
    }
    text.push_str("  ]\n}\n");
    text
}

fn escape_json(text: &str) -> String {
    escape(text).replace('\n', "\\n")
}

/// Every Android permission the declared services need, in order and without
/// repeats.
pub fn android_permissions(capabilities: &Capabilities<'_>) -> Vec<&'static str> {
    let mut named = BTreeSet::new();
    for entry in capabilities.uses {
        for permission in entry.service.android_permissions() {
            named.insert(*permission);
        }
    }
    named.into_iter().collect()
}

/// The Android manifest fragment the declaration becomes.
pub fn android_manifest(capabilities: &Capabilities<'_>) -> String {
    let mut text = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">\n",
    );
    for permission in android_permissions(capabilities) {
        let _ = writeln!(
            text,
            "    <uses-permission android:name=\"{permission}\" />"
        );
    }
    for demand in capabilities.demands {
        let _ = writeln!(
            text,
            "    <uses-feature android:name=\"{}\" android:required=\"true\" />",
            demand.android_feature()
        );
    }
    text.push_str("</manifest>\n");
    text
}

/// The Apple usage descriptions the declaration becomes, as a property list
/// to merge into an `Info.plist`.
///
/// It is a property list of its own, so an Apple build merges it with one
/// line and no tool beyond the ones macOS ships:
///
/// ```text
/// /usr/libexec/PlistBuddy -c "Merge target/cranpose/my-app-usage.plist" MyApp.app/Info.plist
/// ```
pub fn apple_usage(capabilities: &Capabilities<'_>) -> String {
    let mut text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    for entry in capabilities.uses {
        let (Some(key), Some(reason)) = (entry.service.apple_key(), entry.reason) else {
            continue;
        };
        let _ = writeln!(text, "\t<key>{key}</key>");
        let _ = writeln!(text, "\t<string>{}</string>", escape_xml(reason));
    }
    text.push_str("</dict>\n</plist>\n");
    text
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
#[path = "tests/capabilities_tests.rs"]
mod tests;
