use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use super::{MessageSignature, catalog_signatures};
use crate::{Catalog, Locale, Resource};

/// A catalog file discovered under `<root>/<locale>/<namespace>.ftl`.
pub struct CatalogFile {
    /// Absolute path, used to track rebuilds and report errors.
    pub path: PathBuf,
    /// Validated locale directory name.
    pub locale: String,
    /// Package namespace taken from the file stem.
    pub namespace: String,
    /// Complete UTF-8 contents.
    pub source: String,
}

/// Reads catalogs in deterministic order without following directory symlinks.
pub fn load_catalogs(root: &Path) -> Result<Vec<CatalogFile>, String> {
    let mut files = Vec::new();
    for locale_dir in fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let locale_dir = locale_dir.map_err(|error| error.to_string())?;
        if !locale_dir
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let locale = Locale::parse(&locale_dir.file_name().to_string_lossy())
            .map_err(|error| error.to_string())?
            .to_string();
        load_locale(&locale_dir.path(), &locale, &mut files)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn load_locale(directory: &Path, locale: &str, files: &mut Vec<CatalogFile>) -> Result<(), String> {
    let start = files.len();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "ftl") {
            continue;
        }
        let namespace = path
            .file_stem()
            .ok_or("missing namespace")?
            .to_string_lossy()
            .into_owned();
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        files.push(CatalogFile {
            path: path.canonicalize().map_err(|error| error.to_string())?,
            locale: locale.to_owned(),
            namespace,
            source,
        });
    }
    if files.len() == start {
        return Err(format!(
            "{} contains no Fluent catalogs",
            directory.display()
        ));
    }
    Ok(())
}

/// Validates catalogs and returns missing-translation diagnostics.
/// Namespaces without a source-language file are allowed for dependency overrides;
/// provide that file as well to validate their argument contracts.
pub fn validate_catalogs(files: &[CatalogFile], fallback: &str) -> Result<Vec<String>, String> {
    let fallback = Locale::parse(fallback)
        .map_err(|error| error.to_string())?
        .to_string();
    let resources: Vec<_> = files
        .iter()
        .map(|file| Resource {
            locale: &file.locale,
            namespace: &file.namespace,
            source: &file.source,
        })
        .collect();
    Catalog::from_resources(&fallback, &resources).map_err(|error| error.to_string())?;
    let mut signatures = BTreeMap::new();
    for file in files {
        let messages = catalog_signatures(&file.source)
            .map_err(|error| format!("{}: {error}", file.path.display()))?;
        signatures.insert((file.locale.as_str(), file.namespace.as_str()), messages);
    }
    for file in files {
        if file.locale == fallback {
            continue;
        }
        let Some(source) = signatures.get(&(fallback.as_str(), file.namespace.as_str())) else {
            continue;
        };
        let translated = &signatures[&(file.locale.as_str(), file.namespace.as_str())];
        check_arguments(file, source, translated)?;
    }
    Ok(missing_messages(files, &signatures, &fallback))
}

type Signatures<'a> = BTreeMap<(&'a str, &'a str), BTreeMap<String, MessageSignature>>;

fn missing_messages(
    files: &[CatalogFile],
    signatures: &Signatures<'_>,
    fallback: &str,
) -> Vec<String> {
    let locales: BTreeSet<_> = files.iter().map(|file| file.locale.as_str()).collect();
    let mut missing = Vec::new();
    for source in files.iter().filter(|file| file.locale == fallback) {
        let original = &signatures[&(fallback, source.namespace.as_str())];
        for locale in locales.iter().copied().filter(|locale| *locale != fallback) {
            let translated = signatures.get(&(locale, source.namespace.as_str()));
            for id in original
                .keys()
                .filter(|id| translated.is_none_or(|messages| !messages.contains_key(*id)))
            {
                missing.push(format!("{locale}/{}.ftl: missing `{id}`", source.namespace));
            }
        }
    }
    missing
}

fn check_arguments(
    file: &CatalogFile,
    source: &BTreeMap<String, MessageSignature>,
    translated: &BTreeMap<String, MessageSignature>,
) -> Result<(), String> {
    for (id, signature) in translated {
        let Some(original) = source.get(id) else {
            return Err(format!("{}: unknown message `{id}`", file.path.display()));
        };
        let unknown: Vec<_> = signature
            .variables
            .difference(&original.variables)
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "{}: `{id}` requires unknown arguments {unknown:?}",
                file.path.display()
            ));
        }
    }
    Ok(())
}
