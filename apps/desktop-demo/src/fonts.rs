/// Font bundle for the demo application.
///
/// - NotoSansMerged: Latin/Greek/Cyrillic + arrows/symbols + Hebrew, Regular weight (OFL 1.1)
/// - NotoSansBold: Latin/Greek/Cyrillic, Bold weight (OFL 1.1)
/// - TwemojiMozilla: COLR+CPAL v0 color emoji (Apache 2.0 / CC-BY 4.0)
pub static DEMO_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/NotoSansMerged.ttf"),
    include_bytes!("../assets/NotoSansBold.ttf"),
    include_bytes!("../assets/TwemojiMozilla.ttf"),
];
