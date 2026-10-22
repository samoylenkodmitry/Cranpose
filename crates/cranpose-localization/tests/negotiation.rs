//! A request gets the most specific catalog language that fits it, whatever
//! order the catalogs come in.

use cranpose_localization::{Catalog, Locale, Resource};

fn shown(catalog: &Catalog, request: &str) -> String {
    let locale = Locale::parse(request).expect("a language tag");
    catalog.negotiate(&[locale]).locale().to_string()
}

#[test]
fn a_script_catalog_wins_over_the_catalog_of_its_language() {
    let tags = ["en", "sr", "sr-Latn", "zh-Hans", "zh-Hant"];
    let resources: Vec<Resource<'_>> = tags
        .iter()
        .map(|tag| Resource {
            locale: tag,
            namespace: "app",
            source: "hello = Hello\n",
        })
        .collect();
    let catalog = Catalog::from_resources("en", &resources).expect("valid catalogs");
    for (request, expected) in [
        ("sr-Latn-RS", "sr-Latn"),
        ("sr-Latn", "sr-Latn"),
        ("sr-RS", "sr"),
        ("sr-Cyrl-RS", "sr"),
        ("sr", "sr"),
        ("zh-HK", "zh-Hant"),
        ("zh-CN", "zh-Hans"),
        ("de-AT", "en"),
    ] {
        assert_eq!(shown(&catalog, request), expected, "{request}");
    }
    let library = Catalog::from_resources("en", &resources[..2]).expect("valid catalogs");
    let layered = Catalog::from_resources("en", &resources[..1])
        .expect("valid catalogs")
        .with_fallback(&library)
        .with_fallback(&catalog);
    assert_eq!(shown(&layered, "sr-Latn-RS"), "sr-Latn");
}
