//! Parses every `api/current.txt` metalava signature file found under a
//! given AndroidX Compose checkout into structured JSON: one entry per
//! class declaration, method, field, property, constructor, and enum
//! constant. See `docs/compose_api_parity.md`.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Serialize, Clone)]
struct Entry {
    package: String,
    class: String,
    class_kind: String,
    member_kind: String,
    name: String,
    raw: String,
    is_static: bool,
    deprecated: bool,
    experimental: bool,
}

/// Removes every `@Annotation` or `@Annotation(...)` token from `line`,
/// with one exception: the bare keyword `@interface` (Java's
/// annotation-type declaration) is left in place because it is part of the
/// class-kind grammar, not an annotation usage.
fn strip_annotations(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let start = i;
            let mut j = i + 1;
            while j < chars.len()
                && (chars[j].is_alphanumeric() || chars[j] == '.' || chars[j] == '_')
            {
                j += 1;
            }
            let token: String = chars[start..j].iter().collect();
            if token == "@interface" {
                out.push_str(&token);
                i = j;
                continue;
            }
            if j < chars.len() && chars[j] == '(' {
                let mut depth = 1;
                let mut k = j + 1;
                while k < chars.len() && depth > 0 {
                    match chars[k] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    k += 1;
                }
                i = k;
            } else {
                i = j;
            }
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

const CLASS_MODIFIERS: &[&str] = &[
    "public",
    "protected",
    "internal",
    "private",
    "static",
    "final",
    "abstract",
    "sealed",
    "default",
    "open",
    "inline",
    "value",
    "fun",
    "external",
    "operator",
    "infix",
    "suspend",
    "companion",
    "data",
    "exhaustive",
    "nonexhaustive",
];

fn match_class_decl(stripped: &str) -> Option<(String, String)> {
    let tokens: Vec<&str> = stripped.trim().split(' ').collect();
    let mut i = 0;
    while i < tokens.len() && CLASS_MODIFIERS.contains(&tokens[i]) {
        i += 1;
    }
    let kind = *tokens.get(i)?;
    if !matches!(
        kind,
        "class" | "interface" | "enum" | "@interface" | "object"
    ) {
        return None;
    }
    let raw_name = tokens.get(i + 1)?;
    let name = raw_name
        .split(['<', ','])
        .next()
        .unwrap_or(raw_name)
        .to_string();
    Some((kind.to_string(), name))
}

/// A `typealias` is a single-line, package-level declaration -- unlike
/// `class`/`interface`/`enum`, it never opens a `{ ... }` block, so a match
/// here must not become the parser's `cur_class`.
fn match_typealias(stripped: &str) -> Option<String> {
    let tokens: Vec<&str> = stripped.trim().split(' ').collect();
    let mut i = 0;
    while i < tokens.len() && CLASS_MODIFIERS.contains(&tokens[i]) {
        i += 1;
    }
    if tokens.get(i) != Some(&"typealias") {
        return None;
    }
    let raw_name = tokens.get(i + 1)?;
    Some(
        raw_name
            .split(['<', ','])
            .next()
            .unwrap_or(raw_name)
            .to_string(),
    )
}

fn member_name(kind: &str, rest: &str) -> String {
    if (kind == "method" || kind == "ctor")
        && let Some(paren) = rest.find('(')
    {
        let head = &rest[..paren];
        if let Some(name_start) = head.rfind([' ', '>']) {
            return head[name_start + 1..].to_string();
        }
        return head.trim().to_string();
    }
    field_or_property_name(rest)
}

/// A field, property, or enum constant is `<modifiers...> <type> <name>`,
/// optionally followed by `= <default value>` -- the name is the last
/// whitespace-separated token before that, not the first one (which is a
/// modifier keyword such as `public` or `static`).
fn field_or_property_name(rest: &str) -> String {
    let before_eq = rest.split('=').next().unwrap_or(rest).trim();
    before_eq
        .rsplit(' ')
        .next()
        .unwrap_or(before_eq)
        .trim()
        .to_string()
}

fn parse_member(stripped: &str) -> Option<(String, String)> {
    let stripped = stripped.trim();
    let (kind, rest) = stripped.split_once(' ')?;
    if !matches!(
        kind,
        "ctor" | "method" | "field" | "property" | "enum_constant"
    ) {
        return None;
    }
    let semicolon = rest.rfind(';')?;
    let body = rest[..semicolon].trim_end();
    Some((kind.to_string(), body.to_string()))
}

type UnmatchedLine = (usize, String);

fn parse_file(path: &Path) -> Result<(Vec<Entry>, Vec<UnmatchedLine>)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut package = String::new();
    let mut entries = vec![];
    let mut unmatched = vec![];
    let mut cur_class: Option<(String, String)> = None;

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw;
        if line.trim().is_empty() || line.starts_with("// Signature") {
            continue;
        }
        if line.starts_with("package ") {
            let stripped_pkg = strip_annotations(line);
            if let Some(rest) = stripped_pkg.strip_prefix("package ")
                && let Some(name) = rest.trim().strip_suffix(" {")
            {
                package = name.to_string();
            }
            continue;
        }
        if line == "}" {
            continue;
        }
        if line == "  }" {
            cur_class = None;
            continue;
        }
        let stripped = strip_annotations(line);
        let had_annotation = stripped != line;

        if cur_class.is_none()
            && line.starts_with("  ")
            && let Some(name) = match_typealias(&stripped)
        {
            entries.push(Entry {
                package: package.clone(),
                class: name.clone(),
                class_kind: "typealias".to_string(),
                member_kind: "class_decl".to_string(),
                name,
                raw: line.trim().to_string(),
                is_static: false,
                deprecated: line.contains("@Deprecated"),
                experimental: had_annotation && line.contains("Experimental"),
            });
            continue;
        }
        if cur_class.is_none()
            && line.starts_with("  ")
            && let Some((kind, name)) = match_class_decl(&stripped)
        {
            entries.push(Entry {
                package: package.clone(),
                class: name.clone(),
                class_kind: kind.clone(),
                member_kind: "class_decl".to_string(),
                name: name.clone(),
                raw: line.trim().to_string(),
                is_static: false,
                deprecated: line.contains("@Deprecated"),
                experimental: had_annotation && line.contains("Experimental"),
            });
            cur_class = Some((kind, name));
            continue;
        }
        if let Some((class_kind, class_name)) = &cur_class
            && line.starts_with("    ")
            && let Some((kind, rest)) = parse_member(&stripped)
        {
            let name = member_name(&kind, &rest);
            entries.push(Entry {
                package: package.clone(),
                class: class_name.clone(),
                class_kind: class_kind.clone(),
                member_kind: kind,
                name,
                raw: line.trim().to_string(),
                is_static: rest.split_whitespace().any(|t| t == "static"),
                deprecated: line.contains("@Deprecated"),
                experimental: had_annotation && line.contains("Experimental"),
            });
            continue;
        }
        unmatched.push((lineno + 1, line.to_string()));
    }
    Ok((entries, unmatched))
}

fn find_current_txt_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = vec![];
    walk(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.file_name().and_then(|n| n.to_str()) == Some("current.txt")
            && path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some("api")
        {
            out.push(path);
        }
    }
    Ok(())
}

fn module_name(compose_root: &Path, api_current_txt: &Path) -> String {
    let rel = api_current_txt
        .strip_prefix(compose_root)
        .unwrap_or(api_current_txt);
    let module_dir = rel.parent().and_then(|p| p.parent()).unwrap_or(rel);
    module_dir.display().to_string().replace('\\', "/")
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => root = Some(PathBuf::from(args.next().context("--root needs a value")?)),
            "--out" => out_path = Some(PathBuf::from(args.next().context("--out needs a value")?)),
            other => bail!("unknown argument: {other}"),
        }
    }
    let root =
        root.context("--root is required (path to the androidx compose/ checkout directory)")?;
    let out_path = out_path.context("--out is required")?;

    let files = find_current_txt_files(&root)?;
    let mut modules = serde_json::Map::new();
    let mut total = 0usize;
    let mut total_unmatched = 0usize;
    for f in &files {
        let module = module_name(&root, f);
        let (entries, unmatched) = parse_file(f)?;
        total += entries.len();
        total_unmatched += unmatched.len();
        for (lineno, line) in &unmatched {
            eprintln!("UNMATCHED {}:{}: {}", f.display(), lineno, line);
        }
        modules.insert(module, serde_json::to_value(&entries)?);
    }
    let out = serde_json::json!({ "modules": modules, "total_entries": total });
    std::fs::write(&out_path, serde_json::to_string_pretty(&out)?)
        .with_context(|| format!("writing {}", out_path.display()))?;
    println!(
        "parsed {} modules, {} entries, {} unmatched lines -> {}",
        files.len(),
        total,
        total_unmatched,
        out_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_annotations_removes_simple_and_parenthesized_forms() {
        assert_eq!(
            strip_annotations("@Deprecated public fun foo();"),
            "public fun foo();"
        );
        assert_eq!(
            strip_annotations("@RequiresApi(android.os.Build.VERSION_CODES.O) public fun foo();"),
            "public fun foo();"
        );
    }

    #[test]
    fn strip_annotations_keeps_the_interface_keyword() {
        assert_eq!(
            strip_annotations("public @interface Foo {"),
            "public @interface Foo {"
        );
    }

    #[test]
    fn match_class_decl_handles_out_of_order_and_kotlin_modifiers() {
        assert_eq!(
            match_class_decl("public abstract static class Modifier.Node"),
            Some(("class".to_string(), "Modifier.Node".to_string()))
        );
        assert_eq!(
            match_class_decl("public abstract sealed nonexhaustive class Group"),
            Some(("class".to_string(), "Group".to_string()))
        );
        assert_eq!(
            match_class_decl("public sealed exhaustive interface GridTrackSpec"),
            Some(("interface".to_string(), "GridTrackSpec".to_string()))
        );
        assert_eq!(
            match_class_decl("public static final value class StartOffset"),
            Some(("class".to_string(), "StartOffset".to_string()))
        );
        assert_eq!(
            match_class_decl("public fun interface Easing"),
            Some(("interface".to_string(), "Easing".to_string()))
        );
    }

    #[test]
    fn match_class_decl_strips_generic_parameters_from_the_name() {
        assert_eq!(
            match_class_decl("public final class Animatable<T, V>"),
            Some(("class".to_string(), "Animatable".to_string()))
        );
    }

    #[test]
    fn match_typealias_reads_the_alias_name_and_does_not_open_a_block() {
        assert_eq!(
            match_typealias("public typealias CompositeKeyHashCode = long"),
            Some("CompositeKeyHashCode".to_string())
        );
        assert_eq!(match_typealias("public final class Foo"), None);
    }

    #[test]
    fn member_name_reads_a_field_names_last_token_not_its_leading_modifier() {
        assert_eq!(
            member_name(
                "field",
                "public static final int DefaultDurationMillis = 300"
            ),
            "DefaultDurationMillis"
        );
        assert_eq!(member_name("property", "final int Bottom"), "Bottom");
    }

    #[test]
    fn member_name_reads_a_methods_name_before_its_parameter_list() {
        assert_eq!(
            member_name(
                "method",
                "public static androidx.compose.ui.graphics.Canvas Canvas(android.graphics.Canvas c)"
            ),
            "Canvas"
        );
        assert_eq!(
            member_name("method", "public default <R> R foldIn(R initial)"),
            "foldIn"
        );
    }

    #[test]
    fn member_name_keeps_a_kotlin_mangled_value_class_method_name_intact() {
        assert_eq!(
            member_name("method", "public int component1-D9Ej5fM()"),
            "component1-D9Ej5fM"
        );
    }

    #[test]
    fn parse_member_splits_kind_from_body_and_drops_the_trailing_comment() {
        let (kind, body) =
            parse_member("field public static final int DefaultDurationMillis = 300; // 0x12c")
                .unwrap();
        assert_eq!(kind, "field");
        assert_eq!(body, "public static final int DefaultDurationMillis = 300");
    }
}
