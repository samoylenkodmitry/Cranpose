use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use fluent_syntax::ast::{Entry, PatternElement};

use super::{CatalogFile, load_catalogs, load_locale_manifest};

type NativeLabels = BTreeMap<String, (String, String)>;
type NativeMessages = BTreeMap<(String, String), String>;

/// Renders Android's ordered app-specific locale configuration.
pub fn generate_android_locale_config(manifest: &Path) -> Result<String, String> {
    let languages = load_locale_manifest(manifest)?;
    let mut output = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<locale-config xmlns:android=\"http://schemas.android.com/apk/res/android\">\n",
    );
    for language in languages {
        output.push_str(&format!(
            "    <locale android:name=\"{}\" />\n",
            language.tag
        ));
    }
    output.push_str("</locale-config>\n");
    Ok(output)
}

/// Replaces `CFBundleLocalizations` in an iOS property list from the manifest.
pub fn update_ios_localizations(plist: &str, manifest: &Path) -> Result<String, String> {
    let languages = load_locale_manifest(manifest)?;
    let key = "<key>CFBundleLocalizations</key>";
    let start = plist
        .find(key)
        .ok_or("Info.plist has no CFBundleLocalizations key")?;
    let array_start = plist[start + key.len()..]
        .find("<array>")
        .map(|offset| start + key.len() + offset)
        .ok_or("CFBundleLocalizations has no array")?;
    let array_end = plist[array_start..]
        .find("</array>")
        .map(|offset| array_start + offset + "</array>".len())
        .ok_or("CFBundleLocalizations array is not closed")?;
    let mut array = String::from("<array>\n");
    for language in languages {
        array.push_str(&format!("\t\t<string>{}</string>\n", language.tag));
    }
    array.push_str("\t</array>");
    let mut output = String::with_capacity(plist.len() + array.len());
    output.push_str(&plist[..array_start]);
    output.push_str(&array);
    output.push_str(&plist[array_end..]);
    Ok(output)
}

/// Builds language labels for native accessibility tests from Fluent catalogs.
pub fn generate_native_fixtures(
    manifest: &Path,
    catalogs: &Path,
    library_catalogs: Option<&Path>,
    config: &Path,
) -> Result<String, String> {
    let languages = load_locale_manifest(manifest)?;
    let labels = load_native_labels(config)?;
    let requested: BTreeSet<_> = labels.values().cloned().collect();
    let namespaces: BTreeSet<_> = requested
        .iter()
        .map(|(namespace, _)| namespace.as_str())
        .collect();
    let files = load_catalogs(catalogs)?;
    let library_files = library_catalogs
        .map(load_catalogs)
        .transpose()?
        .unwrap_or_default();
    let mut output = String::from("[\n");
    for (language_index, language) in languages.iter().enumerate() {
        let messages = messages_for_language(
            &language.tag,
            &namespaces,
            &requested,
            &files,
            &library_files,
        )?;
        append_native_language(&mut output, language_index, language, &labels, &messages)?;
    }
    output.push_str("\n]\n");
    Ok(output)
}

fn load_native_labels(config: &Path) -> Result<NativeLabels, String> {
    let config_source = fs::read_to_string(config).map_err(|error| error.to_string())?;
    let config_value: toml::Table = config_source
        .parse::<toml::Table>()
        .map_err(|error| error.to_string())?;
    let fixture = config_value
        .get("native_fixture")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "config needs a [native_fixture] table".to_owned())?;
    let namespace = fixture
        .get("namespace")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "[native_fixture] needs a `namespace`".to_owned())?;
    fixture
        .iter()
        .filter(|(key, _)| *key != "namespace")
        .map(|(key, value)| {
            value
                .as_str()
                .map(|id| {
                    let (entry_namespace, entry_id) = id.split_once('/').unwrap_or((namespace, id));
                    (
                        key.to_owned(),
                        (entry_namespace.to_owned(), entry_id.to_owned()),
                    )
                })
                .ok_or_else(|| format!("native fixture `{key}` must be a message ID"))
        })
        .collect::<Result<NativeLabels, _>>()
}

fn messages_for_language(
    language: &str,
    namespaces: &BTreeSet<&str>,
    requested: &BTreeSet<(String, String)>,
    files: &[CatalogFile],
    library_files: &[CatalogFile],
) -> Result<NativeMessages, String> {
    let mut messages = BTreeMap::new();
    for namespace in namespaces {
        let file = files
            .iter()
            .find(|file| file.locale == language && file.namespace.as_str() == *namespace)
            .or_else(|| {
                library_files
                    .iter()
                    .find(|file| file.locale == language && file.namespace.as_str() == *namespace)
            })
            .ok_or_else(|| {
                format!("missing catalog {language}/{namespace}.ftl for configured fixture labels")
            })?;
        collect_catalog_messages(file, namespace, requested, &mut messages)?;
    }
    Ok(messages)
}

fn collect_catalog_messages(
    file: &CatalogFile,
    namespace: &str,
    requested: &BTreeSet<(String, String)>,
    messages: &mut NativeMessages,
) -> Result<(), String> {
    let resource = fluent_syntax::parser::parse(file.source.as_str())
        .map_err(|(_, errors)| format!("{}: {errors:?}", file.path.display()))?;
    for entry in &resource.body {
        let Entry::Message(message) = entry else {
            continue;
        };
        let Some(pattern) = message.value.as_ref() else {
            continue;
        };
        let message_key = (namespace.to_owned(), message.id.name.to_owned());
        if !requested.contains(&message_key) {
            continue;
        }
        let text = pattern
            .elements
            .iter()
            .map(|element| match element {
                PatternElement::TextElement { value } => Ok(value.to_owned()),
                PatternElement::Placeable { .. } => Err(format!(
                    "native fixture message `{}` has a placeable and needs an explicit static label",
                    message.id.name
                )),
            })
            .collect::<Result<String, _>>()?;
        messages.insert(message_key, text);
    }
    Ok(())
}

fn append_native_language(
    output: &mut String,
    language_index: usize,
    language: &super::LocaleMetadata,
    labels: &NativeLabels,
    messages: &NativeMessages,
) -> Result<(), String> {
    if language_index > 0 {
        output.push_str(",\n");
    }
    output.push_str("  {\n    \"tag\": ");
    json_string(output, &language.tag);
    output.push_str(",\n    \"name\": ");
    json_string(output, &language.native_name);
    for (key, (entry_namespace, entry_id)) in labels {
        let text = messages
            .get(&(entry_namespace.clone(), entry_id.clone()))
            .ok_or_else(|| {
                format!(
                    "{}/{} is missing message `{entry_id}`",
                    language.tag, entry_namespace
                )
            })?;
        output.push_str(",\n    ");
        json_string(output, key);
        output.push_str(": ");
        json_string(output, text);
    }
    output.push_str("\n  }");
    Ok(())
}

fn json_string(output: &mut String, value: &str) {
    output.push_str(&serde_json::to_string(value).expect("string serialization is infallible"));
}
