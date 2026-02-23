/// Font bundle for the demo application.
///
/// - NotoSansMerged: Latin/Greek/Cyrillic + arrows/symbols + Hebrew (OFL 1.1)
/// - TwemojiMozilla: COLR+CPAL v0 color emoji (Apache 2.0 / CC-BY 4.0)
pub static DEMO_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/NotoSansMerged.ttf"),
    include_bytes!("../assets/TwemojiMozilla.ttf"),
];
