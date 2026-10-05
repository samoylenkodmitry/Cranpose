use std::{collections::BTreeMap, env, fs, path::Path};

use anyhow::{Context, Result, bail};
use cranpose_localization::tooling::{
    audit_ui_sources, extract_sources, generate_android_locale_config, generate_native_fixtures,
    load_catalogs, load_locale_manifest, merge_source_catalog, update_ios_localizations,
    validate_catalogs,
};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
        return Ok(());
    };
    if command == "--help" {
        usage();
        return Ok(());
    }
    let options = options(args)?;
    match command.as_str() {
        "extract" => extract(&options),
        "check" => check(&options),
        "audit" => audit(&options),
        "native-fixtures" => native_fixtures(&options),
        _ => bail!("unknown command `{command}`; use --help"),
    }
}

fn usage() {
    println!(
        "cranpose-l10n extract --source src --output locales/en/my-app.ftl [--source-locale en] [--check]\ncranpose-l10n check --catalogs locales --fallback en [--allow-missing] [--manifest locales/localization.toml]\ncranpose-l10n audit --source src --config localization.toml [--check]\ncranpose-l10n native-fixtures --manifest locales/localization.toml --catalogs locales --config localization.toml --output android.json [--library-catalogs framework-locales] [--ios-output ios.json] [--android-locales locales_config.xml] [--ios-plist Info.plist] [--check]"
    );
}

fn options(mut args: impl Iterator<Item = String>) -> Result<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    while let Some(name) = args.next() {
        let value = match name.as_str() {
            "--check" | "--allow-missing" => String::new(),
            "--source" | "--output" | "--ios-output" | "--android-locales" | "--ios-plist"
            | "--source-locale" | "--catalogs" | "--library-catalogs" | "--fallback"
            | "--config" | "--manifest" => args
                .next()
                .with_context(|| format!("missing value for {name}"))?,
            _ => bail!("unknown option `{name}`"),
        };
        if options.insert(name.clone(), value).is_some() {
            bail!("duplicate option `{name}`");
        }
    }
    Ok(options)
}

fn required<'a>(options: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    options
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("missing {name}"))
}

fn extract(options: &BTreeMap<String, String>) -> Result<()> {
    let source = Path::new(required(options, "--source")?);
    let output = Path::new(required(options, "--output")?);
    let locale = options.get("--source-locale").map_or("en", String::as_str);
    let extracted = extract_sources(source, locale).map_err(anyhow::Error::msg)?;
    let previous = match fs::read_to_string(output) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let text = merge_source_catalog(&previous, &extracted).map_err(anyhow::Error::msg)?;
    if options.contains_key("--check") {
        if previous != text {
            bail!(
                "{} is out of date; rerun extract without --check and review source changes",
                output.display()
            );
        }
    } else {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output, text).with_context(|| output.display().to_string())?;
        println!(
            "Updated {}; review changed IDs before updating translations.",
            output.display()
        );
    }
    Ok(())
}

fn check(options: &BTreeMap<String, String>) -> Result<()> {
    let files =
        load_catalogs(Path::new(required(options, "--catalogs")?)).map_err(anyhow::Error::msg)?;
    let missing =
        validate_catalogs(&files, required(options, "--fallback")?).map_err(anyhow::Error::msg)?;
    for diagnostic in &missing {
        eprintln!("{diagnostic}");
    }
    if !missing.is_empty() && !options.contains_key("--allow-missing") {
        bail!("{} missing translations", missing.len());
    }
    println!("Validated {} catalogs.", files.len());
    if let Some(path) = options.get("--manifest") {
        let languages = load_locale_manifest(Path::new(path)).map_err(anyhow::Error::msg)?;
        let catalog_tags: std::collections::BTreeSet<_> =
            files.iter().map(|file| file.locale.as_str()).collect();
        let manifest_tags: std::collections::BTreeSet<_> = languages
            .iter()
            .map(|language| language.tag.as_str())
            .collect();
        if catalog_tags != manifest_tags {
            bail!("manifest locale tags must match catalog directories");
        }
        println!(
            "Validated {} ordered language declarations.",
            languages.len()
        );
    }
    Ok(())
}

fn audit(options: &BTreeMap<String, String>) -> Result<()> {
    let findings = audit_ui_sources(
        Path::new(required(options, "--source")?),
        Path::new(required(options, "--config")?),
    )
    .map_err(anyhow::Error::msg)?;
    println!("[");
    for (index, finding) in findings.iter().enumerate() {
        if index > 0 {
            println!(",");
        }
        print!(
            "  {{\"file\":{},\"line\":{},\"column\":{},\"text\":{}}}",
            serde_json::to_string(&finding.file)?,
            finding.line,
            finding.column,
            serde_json::to_string(&finding.text)?
        );
    }
    println!("\n]");
    if !findings.is_empty() && options.contains_key("--check") {
        bail!("{} untranslated UI text literals found", findings.len());
    }
    Ok(())
}

fn native_fixtures(options: &BTreeMap<String, String>) -> Result<()> {
    let content = generate_native_fixtures(
        Path::new(required(options, "--manifest")?),
        Path::new(required(options, "--catalogs")?),
        options.get("--library-catalogs").map(Path::new),
        Path::new(required(options, "--config")?),
    )
    .map_err(anyhow::Error::msg)?;
    write_native_fixture_outputs(options, &content)?;
    write_platform_locale_outputs(options)?;
    println!("Generated native fixtures for the declared languages.");
    Ok(())
}

fn write_native_fixture_outputs(options: &BTreeMap<String, String>, content: &str) -> Result<()> {
    for name in ["--output", "--ios-output"] {
        let Some(path) = options.get(name) else {
            continue;
        };
        write_generated(Path::new(path), content, options.contains_key("--check"))?;
    }
    Ok(())
}

fn write_platform_locale_outputs(options: &BTreeMap<String, String>) -> Result<()> {
    if let Some(path) = options.get("--android-locales") {
        let content = generate_android_locale_config(Path::new(required(options, "--manifest")?))
            .map_err(anyhow::Error::msg)?;
        write_generated(Path::new(path), &content, options.contains_key("--check"))?;
    }
    if let Some(path) = options.get("--ios-plist") {
        let path = Path::new(path);
        let original = fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let content =
            update_ios_localizations(&original, Path::new(required(options, "--manifest")?))
                .map_err(anyhow::Error::msg)?;
        write_generated(path, &content, options.contains_key("--check"))?;
    }
    Ok(())
}

fn write_generated(path: &Path, content: &str, check: bool) -> Result<()> {
    if check {
        let existing = fs::read_to_string(path).with_context(|| path.display().to_string())?;
        if existing != content {
            bail!("{} is stale; rerun native-fixtures", path.display());
        }
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content).with_context(|| path.display().to_string())?;
    }
    Ok(())
}
