//! What the Mac asks the application to open reaches the composition through
//! the application delegate: a link in a scheme as an opened link, and a
//! `file:` URL as incoming content. Elsewhere the runner has nothing to
//! check.

#[cfg(target_os = "macos")]
pub(crate) fn main() {
    use cranpose_core::{CollectEvents, mutableStateOf, remember};
    use cranpose_services::{rememberIncomingContent, rememberOpenedLinks};
    use cranpose_testing::{find_in_semantics, find_text_exact};
    use cranpose_ui::{Modifier, Text, TextStyle};

    use crate::{robot_exit, robot_launch, text_input_robot_helpers};

    env_logger::init();
    let document = std::env::temp_dir().join("cranpose-open-urls.txt");
    if let Err(error) = std::fs::write(&document, b"opened") {
        robot_exit::fail_without_shutdown(&format!("writing {}: {error}", document.display()));
    }
    let document_url = format!("file://{}", document.display());
    robot_launch::launch("Open URLs", 400, 200)
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(60);
            let shows = |expected: &str| {
                text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                    find_in_semantics(robot, |element| find_text_exact(element, expected))
                })
                .is_some()
            };
            if !shows("link none, file none") {
                robot_exit::fail(&robot, "the screen never showed its first state");
            }
            if let Err(error) = robot.open_urls(&["cranpose-demo://item/7", &document_url]) {
                robot_exit::fail(&robot, &format!("opening the URLs failed: {error}"));
            }
            if !shows("link cranpose-demo://item/7, file cranpose-open-urls.txt") {
                robot_exit::fail(&robot, "the delegate did not hand the URLs on");
            }
            println!("PASS: the link and the document reached the composition");
            let _ = std::fs::remove_file(&document);
            let _ = robot.exit();
        })
        .run(|| {
            let link = remember(|| mutableStateOf("none".to_owned())).with(|state| *state);
            let file = remember(|| mutableStateOf("none".to_owned())).with(|state| *state);
            CollectEvents(rememberOpenedLinks(), (), move |opened| link.set(opened));
            CollectEvents(rememberIncomingContent(), (), move |item| {
                file.set(item.display_name());
            });
            Text(
                format!("link {}, file {}", link.get(), file.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
        });
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn main() {
    println!("SKIP: only macOS hands opened URLs to an application delegate");
}
