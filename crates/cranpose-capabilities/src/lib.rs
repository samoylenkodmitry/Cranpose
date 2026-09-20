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
mod tests {
    use super::*;

    const READS: &str = "Reads a receipt with the camera.";

    const SCANNER: Capabilities<'static> = Capabilities {
        uses: &[Use::camera(READS), Use::notifications(), Use::billing()],
        demands: &[],
    };

    #[test]
    fn a_service_with_a_sentence_carries_it() {
        assert_eq!(SCANNER.reason_for(Service::Camera), Some(READS));
        assert_eq!(SCANNER.reason_for(Service::Notifications), None);
        assert!(SCANNER.has(Service::Billing));
        assert!(!SCANNER.has(Service::Haptics));
    }

    const BOTH_SERVICES: [Use; 2] = [Use::background(), Use::media()];
    const ONE_HAPTIC: [Use; 1] = [Use::haptics()];
    const WATCH: [Demand; 1] = [Demand::Watch];
    const ONE_CAMERA: [Use; 1] = [Use::camera(READS)];
    const CAMERA_HARDWARE: [Demand; 1] = [Demand::Camera];

    #[test]
    fn android_permissions_come_from_the_services_without_repeats() {
        let both = Capabilities {
            uses: &BOTH_SERVICES,
            demands: &[],
        };
        assert_eq!(
            android_permissions(&both),
            vec![
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_DATA_SYNC",
                "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK",
            ]
        );
    }

    #[test]
    fn the_manifest_names_the_permissions_and_the_demanded_hardware() {
        let watch = Capabilities {
            uses: &ONE_HAPTIC,
            demands: &WATCH,
        };
        let text = android_manifest(&watch);
        assert!(text.contains("<uses-permission android:name=\"android.permission.VIBRATE\" />"));
        assert!(text.contains(
            "<uses-feature android:name=\"android.hardware.type.watch\" android:required=\"true\" />"
        ));
    }

    #[test]
    fn the_build_names_where_the_declaration_goes() {
        let named = shared_dir(
            Path::new("/checkout/crates/app"),
            Some(OsString::from("/checkout/target/cranpose")),
        );
        assert_eq!(named, PathBuf::from("/checkout/target/cranpose"));
    }

    #[test]
    fn an_empty_name_is_no_name() {
        let crate_dir = Path::new("/checkout/crates/app");
        assert_eq!(
            shared_dir(crate_dir, Some(OsString::new())),
            shared_dir(crate_dir, None)
        );
    }

    #[test]
    fn apple_takes_only_the_services_it_shows_a_sentence_for() {
        let text = apple_usage(&SCANNER);
        assert!(text.starts_with("<?xml"));
        assert!(text.contains("<key>NSCameraUsageDescription</key>"));
        assert!(text.contains(READS));
        assert!(text.ends_with("</dict>\n</plist>\n"));
        assert!(!text.contains("Notification"));
    }

    #[test]
    fn the_json_lists_services_permissions_and_demands() {
        let text = json(&Capabilities {
            uses: &ONE_CAMERA,
            demands: &CAMERA_HARDWARE,
        });
        assert!(text.contains("\"name\": \"camera\""));
        assert!(text.contains("\"android.permission.CAMERA\""));
        assert!(text.contains("\"android.hardware.camera\""));
    }

    #[test]
    fn the_generated_rust_rebuilds_the_same_declaration() {
        let text = rust_source(&SCANNER);
        assert!(text.contains("Use::camera(\"Reads a receipt with the camera.\")"));
        assert!(text.contains("Use::notifications()"));
        assert!(text.contains("demands: &[\n        ],"));
    }

    #[test]
    fn a_quote_in_a_sentence_survives_the_generated_rust() {
        let text = constructor(&Use::camera("Reads a \"receipt\"."));
        assert_eq!(text, "camera(\"Reads a \\\"receipt\\\".\")");
    }

    #[test]
    fn the_workspace_is_the_tree_the_platform_builds_know() {
        let crate_dir = Path::new("/w/app");
        let holds = |path: &Path| path == Path::new("/w");
        assert_eq!(found_workspace(crate_dir, &holds), Path::new("/w"));
    }

    #[test]
    fn a_workspace_inside_a_workspace_answers_with_the_outer_one() {
        let crate_dir = Path::new("/w/vendor/thing/crates/one");
        let holds = |path: &Path| path == Path::new("/w") || path == Path::new("/w/vendor/thing");
        assert_eq!(found_workspace(crate_dir, &holds), Path::new("/w"));
    }

    #[test]
    fn a_crate_in_no_workspace_answers_with_itself() {
        let crate_dir = Path::new("/w/single");
        assert_eq!(
            found_workspace(crate_dir, &|_| false),
            Path::new("/w/single")
        );
    }

    #[test]
    fn the_declaration_is_three_files_beside_the_generated_source() {
        let outputs = shared_outputs(Path::new("/w/target/cranpose"), "desktop-app-platform");
        let names: Vec<String> = outputs
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "desktop-app-platform-capabilities.json",
                "desktop-app-platform-permissions.xml",
                "desktop-app-platform-usage.plist",
            ]
        );
    }

    #[test]
    fn every_file_written_outside_out_dir_is_named_to_cargo() {
        let outputs = shared_outputs(Path::new("/w/target/cranpose"), "example");
        let directives = rerun_directives(&outputs);
        assert_eq!(
            directives.len(),
            outputs.len(),
            "a file cargo is not told about is a file nothing will rewrite once it is gone"
        );
        for path in &outputs {
            let expected = format!("cargo::rerun-if-changed={}", path.display());
            assert!(
                directives.contains(&expected),
                "{} is written but never named to cargo: {directives:?}",
                path.display()
            );
        }
    }

    /// The coupling is the guard: a file written outside [`shared_outputs`]
    /// would not appear in [`rerun_directives`] either, and deleting it would
    /// break the Android manifest check permanently in that workspace.
    #[test]
    fn emit_writes_the_shared_files_only_through_the_named_list() {
        let source = include_str!("lib.rs");
        let emit = source
            .split_once("pub fn emit(self)")
            .expect("emit is still a function")
            .1
            .split_once("\n}")
            .expect("emit still ends")
            .0;
        for name in ["-capabilities.json", "-permissions.xml", "-usage.plist"] {
            assert!(
                !emit.contains(name),
                "emit names {name} directly instead of going through shared_outputs, \
                 so cargo is never told the file exists"
            );
        }
        assert!(
            emit.contains("shared_outputs(&shared, &package)")
                && emit.contains("rerun_directives(&outputs)"),
            "emit must write and announce the same list of files"
        );
    }
}
