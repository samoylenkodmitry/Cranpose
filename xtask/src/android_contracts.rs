use std::{
    fs,
    path::{Path, PathBuf},
};

use quick_xml::{events::Event, Reader, XmlVersion};

const ANDROID: &str = "crates/cranpose/android";
const ACTIVITY: &str = "crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java";
const BACKGROUND_SERVICE: &str =
    "crates/cranpose/android/java/dev/cranpose/android/CranposeBackgroundService.java";
const BILLING: &str =
    "crates/cranpose/android/java-billing/dev/cranpose/android/CranposeBilling.java";
const APP_UPDATE: &str =
    "crates/cranpose/android/java-update/dev/cranpose/android/CranposeAppUpdate.java";
const MANIFEST_CHECK: &str = "crates/cranpose/android/cranpose-gradle-plugin/src/main/kotlin/dev/cranpose/gradle/CranposeManifestCheck.kt";
const WIRE_TESTS: &str = "crates/cranpose/src/tests/android_accessibility_wire.rs";
const PARSER_TEST: &str = "apps/android-demo/android/app/src/androidTest/java/com/compose_rs/demo/CranposeAccessibilityParserTest.java";
const APPLICATION_MANIFESTS: [&str; 2] = [
    "apps/android-demo/android/app/src/main/AndroidManifest.xml",
    "apps/isolated-demo/android/app/src/main/AndroidManifest.xml",
];
// Each service an application declares in Rust writes these; an application
// manifest that names one asks for it outside that declaration.
const SERVICE_PERMISSIONS: [&str; 6] = [
    "android.permission.VIBRATE",
    "android.permission.POST_NOTIFICATIONS",
    "android.permission.CAMERA",
    "android.permission.FOREGROUND_SERVICE",
    "android.permission.SYSTEM_ALERT_WINDOW",
    "android.permission.REQUEST_INSTALL_PACKAGES",
];

type Check = fn(&Sources, &mut Vec<String>) -> Result<(), String>;

pub(crate) fn problems(root: &Path) -> Result<Vec<String>, String> {
    let sources = Sources { root };
    let checks: [Check; 7] = [
        record_layout,
        background_service_ask,
        background_service_stop,
        billing_order_id,
        framework_receivers,
        framework_manifests,
        application_manifests,
    ];
    let mut problems = Vec::new();
    for check in checks {
        check(&sources, &mut problems)?;
    }
    Ok(problems)
}

struct Sources<'a> {
    root: &'a Path,
}

impl Sources<'_> {
    fn read(&self, relative: &str) -> Result<String, String> {
        read(&self.root.join(relative))
    }

    fn files(&self, relative: &str, extension: &str) -> Result<Vec<PathBuf>, String> {
        let mut files = Vec::new();
        collect(&self.root.join(relative), extension, &mut files)?;
        files.sort();
        Ok(files)
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn collect(directory: &Path, extension: &str, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !matches!(name, "build" | ".gradle") {
                collect(&path, extension, out)?;
            }
        } else if path.extension().is_some_and(|found| found == extension) {
            out.push(path);
        }
    }
    Ok(())
}

// The body of the first method declared with `signature`, braces included.
fn method_body<'a>(java: &'a str, file: &str, signature: &str) -> Result<&'a str, String> {
    let missing = || format!("{file} no longer declares `{signature}`");
    let body = &java[java.find(signature).ok_or_else(missing)?..];
    let open = body.find('{').ok_or_else(missing)?;
    let mut depth = 0usize;
    for (offset, byte) in body.bytes().enumerate().skip(open) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&body[..=offset]);
                }
            }
            _ => {}
        }
    }
    Err(format!("`{signature}` in {file} does not close"))
}

// True when every needle is in `text` and each one starts after the one before.
fn in_order(text: &str, needles: &[&str]) -> bool {
    let mut from = 0;
    needles
        .iter()
        .all(|needle| match text[from..].find(needle) {
            Some(at) => {
                from += at + needle.len();
                true
            }
            None => false,
        })
}

fn quoted_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let rest = &text[text.find(marker)? + marker.len()..];
    let rest = &rest[rest.find('"')? + 1..];
    Some(&rest[..rest.find('"')?])
}

// The record layout has three copies: the kinds the Rust wire tests decode the
// encoder's output with, the order the Android host reads fields in, and the
// records the instrumentation test builds by hand. #1595 changed the first two,
// and the nightly emulator job failed on the third until #1635.
fn record_layout(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    let wire_tests = sources.read(WIRE_TESTS)?;
    let encoded = quoted_after(&wire_tests, "const FIELD_KINDS")
        .ok_or_else(|| format!("{WIRE_TESTS} no longer declares FIELD_KINDS"))?;
    let parser_test = sources.read(PARSER_TEST)?;
    let built = quoted_after(&parser_test, "String KINDS")
        .ok_or_else(|| format!("{PARSER_TEST} no longer declares KINDS"))?;
    let activity = sources.read(ACTIVITY)?;
    let reader = method_body(&activity, ACTIVITY, "void read(ByteBuffer in)")?;
    let tokens = [
        ("in.getInt()", 'N'),
        ("flag(in)", 'N'),
        ("in.getFloat()", 'F'),
        ("text(in, ", 'T'),
        ("actions(in, ", 'A'),
    ];
    let mut found: Vec<(usize, char)> = tokens
        .iter()
        .flat_map(|(token, kind)| reader.match_indices(token).map(move |(at, _)| (at, *kind)))
        .collect();
    found.sort_unstable();
    let read: String = found.into_iter().map(|(_, kind)| kind).collect();
    if encoded != read || encoded != built {
        problems.push(format!(
            "the accessibility record layout differs across the JNI boundary:\n    \
             {WIRE_TESTS}: {encoded}\n    {ACTIVITY} reads: {read}\n    {PARSER_TEST}: {built}"
        ));
    }
    Ok(())
}

// A launch with the screen off pauses without resuming; a foreground service
// asked for from that pause is never created, and Android ends the process for
// the missing startForeground (#499).
fn background_service_ask(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    let java = sources.read(ACTIVITY)?;
    let body = |signature| method_body(&java, ACTIVITY, signature);
    let on_pause = body("protected void onPause()")?;
    let ask = body("private void askForCranposeBackgroundService()")?;
    let on_resume = body("protected void onResume()")?;
    let fire = body("private void startCranposeBackgroundService()")?;
    let set_active = body("public void cranposeShowBackgroundWork(String title, int permille)")?;
    let rules = [
        (
            !on_pause.contains("startCranposeBackgroundService(")
                && !on_pause.contains("startForegroundService(")
                && on_pause.contains("askForCranposeBackgroundService("),
            "onPause must ask for the background service only through askForCranposeBackgroundService",
        ),
        (
            ask.contains("if (!cranposeEverResumed)") && ask.contains("postDelayed("),
            "the ask must wait for a lifetime that reached the foreground, on a handler: an EMUI \
             launch delivers pause and resume together",
        ),
        (
            on_resume.contains("cranposeEverResumed = true") && on_resume.contains("removeCallbacks("),
            "onResume must mark the lifetime resumed and cancel a pending ask",
        ),
        (
            fire.contains("if (!cranposeBackgroundActive || !cranposePaused)")
                && fire.contains("CranposeBackgroundService.start(this)"),
            "the deferred ask must re-check at fire time and start through the service's handshake",
        ),
        (
            set_active.contains("if (started && cranposePaused)")
                && set_active.contains("removeCallbacks(")
                && in_order(set_active, &["runOnUiThread(", "cranposeBackgroundActive = active"]),
            "only the start of work may ask, finished work cancels the ask, and the JNI thread \
             writes cranposeBackgroundActive only through runOnUiThread",
        ),
    ];
    for (kept, rule) in rules {
        if !kept {
            problems.push(format!("{ACTIVITY}: {rule}"));
        }
    }
    Ok(())
}

// A Context.stopService that lands before the service's startForeground is a
// deliberate framework kill (#499), and a stop without an accepted start
// reaches ActivityManager on every resume.
fn background_service_stop(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    let activity = sources.read(ACTIVITY)?;
    if activity.contains("stopService(")
        || activity.contains("startForegroundService(")
        || activity.contains("startService(")
        || !activity.contains("CranposeBackgroundService.stop(this)")
    {
        problems.push(format!(
            "{ACTIVITY} must start and stop the background service only through \
             CranposeBackgroundService.start and stop"
        ));
    }
    let service = sources.read(BACKGROUND_SERVICE)?;
    let body = |signature| method_body(&service, BACKGROUND_SERVICE, signature);
    let start = body("static void start(Context context)")?;
    let stop = body("static void stop(Context context)")?;
    let enter = body("private void enterForeground()")?;
    let rules = [
        (
            start.contains("catch (RuntimeException")
                && in_order(start, &["if (started == null)", "obligationArmed = true"])
                && in_order(start, &["if (started == null)", "stopRequested = false"])
                && in_order(start, &["if (started == null)", "startOutstanding = true"]),
            "start must log a refused start and record only a start the platform accepted",
        ),
        (
            stop.contains("if (obligationArmed)")
                && in_order(
                    stop,
                    &[
                        "if (!startOutstanding)",
                        "startOutstanding = false",
                        "context.stopService(",
                    ],
                ),
            "stop must wait for startForeground and make no call without an outstanding start",
        ),
        (
            enter.contains("startForeground(")
                && in_order(enter, &["obligationArmed = false", "if (stopRequested)"]),
            "enterForeground must honour a deferred stop only after startForeground",
        ),
    ];
    for (kept, rule) in rules {
        if !kept {
            problems.push(format!("{BACKGROUND_SERVICE}: {rule}"));
        }
    }
    Ok(())
}

// Nothing but the Java bridge can check the order id that granted an
// entitlement: the Rust side receives whatever the bridge sends.
fn billing_order_id(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    let java = sources.read(BILLING)?;
    let apply = method_body(&java, BILLING, "private int apply(")?;
    let kept = java.contains("purchase.getOrderId()")
        && java.contains("escape(orderId)")
        && apply.contains("owned.clear()")
        && apply.contains("orders.clear()");
    if !kept {
        problems.push(format!(
            "{BILLING} must send Play's order id escaped on each owned row, and replace ownership \
             and order ids together"
        ));
    }
    Ok(())
}

// Before Android 13 any application can send to a receiver registered at run
// time, and an intent taken out of another intent launches whatever the sender
// put there.
fn framework_receivers(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    let java = sources.files(ANDROID, "java")?;
    if java.len() < 5 {
        return Err(format!(
            "found only {} Java files under {ANDROID}",
            java.len()
        ));
    }
    for path in &java {
        let source = read(path)?;
        let file = path.strip_prefix(sources.root).unwrap_or(path).display();
        let runtime_receiver = source
            .match_indices("registerReceiver(")
            .any(|(at, call)| !source[at + call.len()..].trim_start().starts_with("null"));
        if runtime_receiver {
            problems.push(format!(
                "{file} registers a receiver at run time; declare it in the manifest with \
                 android:exported=\"false\" and address it by class"
            ));
        }
        if source.contains("Intent.EXTRA_INTENT") && !path.ends_with("CranposeAppUpdate.java") {
            problems.push(format!(
                "{file} launches an intent taken out of another intent; only the unexported \
                 update receiver may"
            ));
        }
        if source.contains("PackageInstaller") && !path.ends_with("CranposeAppUpdate.java") {
            problems.push(format!(
                "{file} uses PackageInstaller; the installer belongs in java-update, compiled \
                 only into applications that declare the update service"
            ));
        }
    }
    let update = sources.read(APP_UPDATE)?;
    if !update.contains("new Intent(app, CranposeAppUpdate.class)")
        || !update.contains("session != committedSession")
    {
        problems.push(format!(
            "{APP_UPDATE} must report to its receiver by class and act only on the session it committed"
        ));
    }
    let manifest_check = sources.read(MANIFEST_CHECK)?;
    if !manifest_check
        .contains("setAttributeNS(ANDROID_NAMESPACE, \"android:exported\", \"false\")")
    {
        problems.push(format!(
            "{MANIFEST_CHECK} must declare the receivers it adds for a service unexported"
        ));
    }
    Ok(())
}

struct Element {
    name: String,
    attributes: Vec<(String, String)>,
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn elements(path: &Path) -> Result<Vec<Element>, String> {
    let text = read(path)?;
    let mut reader = Reader::from_str(&text);
    let mut elements = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element) | Event::Empty(element)) => {
                let mut attributes = Vec::new();
                for attribute in element.attributes() {
                    let attribute =
                        attribute.map_err(|error| format!("{}: {error}", path.display()))?;
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    attributes.push((
                        String::from_utf8_lossy(attribute.key.as_ref()).into_owned(),
                        value.into_owned(),
                    ));
                }
                elements.push(Element {
                    name: String::from_utf8_lossy(element.name().as_ref()).into_owned(),
                    attributes,
                });
            }
            Ok(Event::Eof) => return Ok(elements),
            Ok(_) => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
}

fn framework_manifests(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    for path in sources.files(&format!("{ANDROID}/manifests"), "xml")? {
        let file = path
            .strip_prefix(sources.root)
            .unwrap_or(&path)
            .display()
            .to_string();
        for element in elements(&path)? {
            if element.name == "receiver" && element.attribute("android:exported") != Some("false")
            {
                problems.push(format!(
                    "{file} declares a receiver other applications can reach"
                ));
            }
            let install = element.name == "uses-permission"
                && element.attribute("android:name")
                    == Some("android.permission.REQUEST_INSTALL_PACKAGES");
            if install {
                problems.push(format!(
                    "{file} asks to install packages; `Use::update()` writes that permission"
                ));
            }
        }
    }
    Ok(())
}

// The Cranpose library contributes the activity, its lib_name, the share
// provider and each service's permissions to every application's manifest.
fn application_manifests(sources: &Sources, problems: &mut Vec<String>) -> Result<(), String> {
    for file in APPLICATION_MANIFESTS {
        for element in elements(&sources.root.join(file))? {
            let name = element.attribute("android:name").unwrap_or_default();
            let owned = match element.name.as_str() {
                "activity" => Some("the framework activity"),
                "meta-data" if name == "android.app.lib_name" => {
                    Some("the cdylib name, which the plugin takes from cranpose { cargoPackage }")
                }
                "provider" if name.ends_with("CranposeShareProvider") => {
                    Some("the framework's share provider")
                }
                "uses-permission" if SERVICE_PERMISSIONS.contains(&name) => {
                    Some("a permission that a service declared in Rust writes")
                }
                _ => None,
            };
            if let Some(owned) = owned {
                problems.push(format!("{file} declares {owned}: `{name}`"));
            }
        }
    }
    Ok(())
}
