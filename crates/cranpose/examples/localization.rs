use cranpose::{liquid::prelude::*, *};

translation_messages!(pub mod messages, "examples/localization/locales/en/cranpose.ftl");

#[composable]
fn LocalizationDemo(catalog: Catalog) {
    let language = rememberMutableStateOf(|| "en");
    let preview = rememberMutableStateOf(PreviewMode::default);
    let locale = Locale::parse(language.value())
        .expect("language picker uses valid tags")
        .with_preview(preview.value());
    ProvideLocalization(&catalog, locale, || {
        LiquidTheme(LiquidThemeSpec::default(), || {
            widgets::popup::PopupHost(move || {
                Column(
                    Modifier::empty().fill_max_size().padding(24.0),
                    ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                    move || {
                        Row(Modifier::empty(), RowSpec::default(), move || {
                            for (tag, label) in [
                                ("en", "English"),
                                ("fr", "Français"),
                                ("sr-Latn", "Srpski"),
                                ("ar", "العربية"),
                            ] {
                                Button(
                                    Modifier::empty().padding(4.0),
                                    ButtonSpec::default(),
                                    move || language.set_value(tag),
                                    move || {
                                        Text(label, Modifier::empty(), TextStyle::default());
                                    },
                                );
                            }
                        });
                        Button(
                            Modifier::empty(),
                            ButtonSpec::default(),
                            move || {
                                preview.update(|mode| {
                                    *mode = match mode {
                                        PreviewMode::None => PreviewMode::Expanded,
                                        PreviewMode::Expanded => PreviewMode::Rtl,
                                        PreviewMode::Rtl => PreviewMode::None,
                                    };
                                })
                            },
                            || {
                                Text(
                                    "Preview: normal / expanded / RTL",
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                        DemoContent();
                    },
                );
            });
        });
    });
}

#[composable]
fn DemoContent() {
    let count = rememberMutableStateOf(|| 1u32);
    let selected = rememberMutableStateOf(|| 0usize);
    let menu = rememberMutableStateOf(|| false);
    let search =
        remember(|| cranpose_foundation::text::TextFieldState::new("")).with(|state| *state);
    Text(
        tr!("Welcome, {name}!", name = "Ana", id = "welcome"),
        Modifier::empty(),
        TextStyle::default(),
    );
    Text(
        messages::files(count.value()),
        Modifier::empty(),
        TextStyle::default(),
    );
    GlassButton(
        Modifier::empty(),
        GlassButtonSpec::glass(),
        move || count.update(|value| *value += 1),
        || {
            GlassButtonLabel(tr!("Add a file", id = "add-file"), GlassButtonSpec::glass());
        },
    );
    LiquidSearchField(
        Modifier::empty().width(320.0),
        search,
        LiquidSearchFieldSpec::default(),
    );
    LiquidDropdownMenu(
        Modifier::empty(),
        menu.value(),
        LiquidDropdownMenuSpec::default(),
        move || menu.set_value(false),
        move || {
            GlassButton(
                Modifier::empty(),
                GlassButtonSpec::glass(),
                move || menu.set_value(true),
                || {
                    GlassButtonLabel(tr!("Actions", id = "actions"), GlassButtonSpec::glass());
                },
            )
        },
        |scope| {
            scope.item(LiquidMenuItem::new(tr!("Save", id = "save")), || {});
        },
    );
    LiquidTabBar(
        Modifier::empty().width(320.0),
        LiquidTabBarSpec::default(),
        selected.value(),
        move |index| selected.set_value(index),
        |scope| {
            scope.tab(liquid::icons::SEARCH, tr!("Files", id = "files-tab"));
            scope.tab(liquid::icons::SEARCH, tr!("Settings", id = "settings-tab"));
        },
    );
}

fn main() -> Result<(), LaunchError> {
    let catalog = translations!("examples/localization/locales", fallback = "en");
    AppLauncher::new()
        .with_title("Cranpose localization")
        .with_size(650, 560)
        .try_run(move || LocalizationDemo(catalog.clone()))
}
