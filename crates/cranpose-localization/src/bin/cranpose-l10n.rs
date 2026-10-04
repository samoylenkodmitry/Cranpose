use std::{collections::BTreeMap, env, fs, path::Path};

use anyhow::{Context, Result, bail};
use cranpose_localization::tooling::{
    extract_sources, load_catalogs, merge_source_catalog, validate_catalogs,
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
        _ => bail!("unknown command `{command}`; use --help"),
    }
}

fn usage() {
    println!(
        "cranpose-l10n extract --source src --output locales/en/my-app.ftl [--source-locale en] [--check]\ncranpose-l10n check --catalogs locales --fallback en [--allow-missing]"
    );
}

fn options(mut args: impl Iterator<Item = String>) -> Result<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    while let Some(name) = args.next() {
        let value = match name.as_str() {
            "--check" | "--allow-missing" => String::new(),
            "--source" | "--output" | "--source-locale" | "--catalogs" | "--fallback" => args
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
    Ok(())
}
