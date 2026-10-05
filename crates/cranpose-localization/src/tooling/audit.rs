use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use syn::{
    Expr, ExprCall, ExprLit, ExprMacro, ExprMethodCall, Lit, Token, parse::Parser,
    punctuated::Punctuated, visit::Visit,
};

/// A string literal found in a configured user-facing UI argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiTextFinding {
    /// Rust source file containing the literal.
    pub file: String,
    /// One-based source line.
    pub line: usize,
    /// One-based source column.
    pub column: usize,
    /// Text that should be reviewed for translation.
    pub text: String,
}

/// Finds plain string literals in built-in or app-configured UI arguments.
pub fn audit_ui_sources(root: &Path, config: &Path) -> Result<Vec<UiTextFinding>, String> {
    let (widgets, ignored) = load_audit_settings(config)?;
    audit_files(root, &widgets, &ignored)
}

type WidgetArguments = BTreeMap<String, Vec<usize>>;

fn load_audit_settings(config: &Path) -> Result<(WidgetArguments, BTreeSet<String>), String> {
    let config_source = fs::read_to_string(config).map_err(|error| error.to_string())?;
    let config_value: toml::Table = config_source
        .parse::<toml::Table>()
        .map_err(|error| error.to_string())?;
    let ignored: BTreeSet<String> = config_value
        .get("audit")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("ignore"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut widgets = builtin_widgets();
    if let Some(entries) = config_value.get("widget").and_then(toml::Value::as_array) {
        for entry in entries {
            let name = entry
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| "each [[widget]] needs a string `name`".to_owned())?;
            let arguments = entry
                .get("arguments")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| format!("widget `{name}` needs an `arguments` array"))?;
            let indices = arguments
                .iter()
                .map(|value| {
                    value
                        .as_integer()
                        .and_then(|value| usize::try_from(value).ok())
                        .ok_or_else(|| {
                            format!("widget `{name}` arguments must be nonnegative integers")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            widgets.insert(name.to_owned(), indices);
        }
    }
    Ok((widgets, ignored))
}

fn audit_files(
    root: &Path,
    widgets: &BTreeMap<String, Vec<usize>>,
    ignored: &BTreeSet<String>,
) -> Result<Vec<UiTextFinding>, String> {
    let mut files = Vec::new();
    visit_rust_files(root, &mut files)?;
    files.sort();
    let mut findings = Vec::new();
    for file in files {
        let source =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let syntax =
            syn::parse_file(&source).map_err(|error| format!("{}: {error}", file.display()))?;
        let mut visitor = UiVisitor {
            file: file.display().to_string(),
            widgets,
            ignored,
            findings: Vec::new(),
            seen: std::collections::BTreeSet::new(),
        };
        visitor.visit_file(&syntax);
        findings.extend(visitor.findings);
    }
    Ok(findings)
}

fn builtin_widgets() -> BTreeMap<String, Vec<usize>> {
    [
        ("Text", vec![0]),
        ("Image", vec![1]),
        ("content_description", vec![0]),
        ("LiquidMenuItem::new", vec![0]),
        ("LiquidTab::new", vec![1]),
        ("IconButton", vec![1]),
        ("IconButtonTinted", vec![1]),
        ("tab", vec![1]),
    ]
    .into_iter()
    .map(|(name, indices)| (name.to_owned(), indices))
    .collect()
}

fn visit_rust_files(path: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)
            .map_err(|error| error.to_string())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        entries.sort();
        for entry in entries {
            if !entry
                .file_name()
                .is_some_and(|name| name == "target" || name == ".git")
            {
                visit_rust_files(&entry, files)?;
            }
        }
    } else if path.extension().is_some_and(|extension| extension == "rs") {
        files.push(path.to_owned());
    }
    Ok(())
}

struct UiVisitor<'a> {
    file: String,
    widgets: &'a BTreeMap<String, Vec<usize>>,
    ignored: &'a std::collections::BTreeSet<String>,
    findings: Vec<UiTextFinding>,
    seen: std::collections::BTreeSet<(usize, usize)>,
}

impl UiVisitor<'_> {
    fn inspect_call(&mut self, name: String, args: &Punctuated<Expr, Token![,]>) {
        let normalized = name
            .rsplit("::")
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("::");
        let indices = self
            .widgets
            .get(&name)
            .or_else(|| self.widgets.get(&normalized));
        let Some(indices) = indices else { return };
        for index in indices {
            if let Some(argument) = args.get(*index) {
                self.inspect_expr(argument);
            }
        }
    }

    fn inspect_expr(&mut self, expression: &Expr) {
        let mut literals = LiteralVisitor {
            file: self.file.clone(),
            ignored: self.ignored,
            findings: Vec::new(),
        };
        literals.visit_expr(expression);
        for finding in literals.findings {
            if self.seen.insert((finding.line, finding.column)) {
                self.findings.push(finding);
            }
        }
        syn::visit::visit_expr(self, expression);
    }
}

impl<'ast> Visit<'ast> for UiVisitor<'_> {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(path) = call.func.as_ref() {
            let name = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            self.inspect_call(name, &call.args);
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let name = call.method.to_string();
        self.inspect_call(name, &call.args);
        syn::visit::visit_expr_method_call(self, call);
    }
}

struct LiteralVisitor<'a> {
    file: String,
    ignored: &'a std::collections::BTreeSet<String>,
    findings: Vec<UiTextFinding>,
}

impl<'ast> Visit<'ast> for LiteralVisitor<'_> {
    fn visit_expr_lit(&mut self, literal: &'ast ExprLit) {
        let Lit::Str(text) = &literal.lit else {
            return;
        };
        let value = text.value();
        let start = text.span().start();
        if has_static_words(&value) && !self.ignored.contains(value.trim()) {
            self.findings.push(UiTextFinding {
                file: self.file.clone(),
                line: start.line,
                column: start.column,
                text: value,
            });
        }
    }

    fn visit_expr_macro(&mut self, mac: &'ast ExprMacro) {
        let name = mac
            .mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if !matches!(name.as_deref(), Some("format" | "format_args" | "concat")) {
            return;
        }
        if let Ok(arguments) =
            Punctuated::<Expr, Token![,]>::parse_terminated.parse2(mac.mac.tokens.clone())
        {
            for argument in arguments {
                self.visit_expr(&argument);
            }
        }
    }
}

fn has_static_words(value: &str) -> bool {
    let mut in_argument = false;
    for character in value.chars() {
        match character {
            '{' => in_argument = true,
            '}' => in_argument = false,
            character if !in_argument && character.is_alphabetic() => return true,
            _ => {}
        }
    }
    false
}
