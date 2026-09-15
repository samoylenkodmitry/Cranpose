use std::path::Path;

const CLASS_NAME: &str = "CranposeSceneDelegate";

fn read(path: &str) -> String {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(crate_dir.join(path)).expect("failed to read the file under test")
}

#[test]
fn scene_delegate_keeps_the_name_apps_declare() {
    let source = read("src/ios_scene.rs");
    assert!(
        source.contains(&format!("#[name = \"{CLASS_NAME}\"]")),
        "ios_scene.rs no longer registers {CLASS_NAME}; every app Info.plist names that class"
    );
}

#[test]
fn the_demo_plist_names_the_registered_delegate() {
    let plist = read("../../apps/ios-demo/ios/CranposeDemo/Info.plist");
    assert!(
        plist.contains("<key>UISceneDelegateClassName</key>"),
        "the demo Info.plist declares no scene delegate; iOS 27 ends an app without one"
    );
    assert!(
        plist.contains(&format!("<string>{CLASS_NAME}</string>")),
        "the demo Info.plist names a delegate class other than {CLASS_NAME}"
    );
}

#[test]
fn the_demo_plist_holds_one_scene_configuration() {
    let plist = read("../../apps/ios-demo/ios/CranposeDemo/Info.plist");
    assert!(plist.contains("<key>UIApplicationSceneManifest</key>"));
    assert!(plist.contains("<key>UIWindowSceneSessionRoleApplication</key>"));
    assert_eq!(
        plist.matches("<key>UISceneDelegateClassName</key>").count(),
        1,
        "more than one scene configuration names a delegate"
    );
}
