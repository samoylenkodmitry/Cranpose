use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::LazyLock,
};

use regex::Regex;

/// `just`'s own recipe-name grammar: a letter or underscore, then
/// letters, digits, underscores or hyphens. Requiring that first
/// character keeps `cargo install just --locked` from reading as an
/// invocation of a recipe named `--locked` -- `-` is a valid character
/// inside a recipe name (`test-shell-helpers`) but never starts one.
static JUST_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bjust\s+([A-Za-z_][A-Za-z0-9_-]*)").expect("JUST_CALL_RE is a valid pattern")
});

/// Every `just <recipe>` name mentioned in `line`, ignoring anything
/// after the name itself -- `just robot-one my_example` yields
/// `robot-one`, so a recipe invoked with arguments is still found.
fn just_recipe_calls(line: &str) -> impl Iterator<Item = String> + '_ {
    JUST_CALL_RE.captures_iter(line).map(|c| c[1].to_owned())
}

/// One `just <recipe>` call found in a workflow's `run:` step, kept with
/// enough context to name where it came from in a failure message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkflowInvocation {
    pub(crate) recipe: String,
    pub(crate) line_number: usize,
    pub(crate) line_text: String,
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

fn collect_workflow_calls(
    line: &str,
    line_number: usize,
    invocations: &mut Vec<WorkflowInvocation>,
) {
    for recipe in just_recipe_calls(line) {
        invocations.push(WorkflowInvocation {
            recipe,
            line_number,
            line_text: line.trim().to_owned(),
        });
    }
}

/// Every `just <recipe>` call a GitHub Actions workflow's `run:` steps
/// make, whether written inline (`run: just fmt-check`) or inside a
/// block scalar (`run: |` followed by indented shell lines). Only
/// `run:` step bodies are scanned, not comments or doc-prose that merely
/// mentions a recipe in backticks, by tracking each `run:` line's own
/// indentation and following YAML's block-scalar rule: the block's
/// content ends at the first non-blank line back at or above that
/// indentation (blank lines inside the block do not end it).
pub(crate) fn parse_workflow_just_invocations(workflow_text: &str) -> Vec<WorkflowInvocation> {
    let lines: Vec<&str> = workflow_text.split('\n').collect();
    let mut invocations = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let indent = leading_spaces(line);
        // `- run: just x` and `run: just x` are the same step to YAML.
        // Matching only the second would make this gate's coverage depend
        // on a formatting convention nothing enforces: one step written
        // with the leading dash would be skipped in silence, and a gate
        // that silently sees nothing reports every repository clean.
        let bare = line.trim_start();
        let bare = bare.strip_prefix("- ").map_or(bare, str::trim_start);
        let Some(rest) = bare.strip_prefix("run:") else {
            index += 1;
            continue;
        };
        let rest = rest.trim();
        let is_block_scalar = rest.is_empty() || rest.starts_with('|') || rest.starts_with('>');
        if !is_block_scalar {
            collect_workflow_calls(rest, index + 1, &mut invocations);
            index += 1;
            continue;
        }
        let mut cursor = index + 1;
        while cursor < lines.len() {
            let body_line = lines[cursor];
            if body_line.trim().is_empty() {
                cursor += 1;
                continue;
            }
            if leading_spaces(body_line) <= indent {
                break;
            }
            collect_workflow_calls(body_line, cursor + 1, &mut invocations);
            cursor += 1;
        }
        index = cursor;
    }
    invocations
}

/// The justfile's own recipe dependency graph: every recipe name it
/// defines, and for each one, the recipes it reaches directly -- both
/// declared prerequisites (`ci: fmt-check typos ...`) and recipes it
/// shells out to by name from its own body. `budgets` runs `just
/// budgets-here` from its body rather than declaring it as a
/// prerequisite, so a reading of only recipe headers would miss that
/// edge and wrongly report `budgets-here`'s dependencies as unreachable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct JustfileGraph {
    pub(crate) recipes: BTreeSet<String>,
    pub(crate) edges: BTreeMap<String, BTreeSet<String>>,
}

/// Splits a recipe header into its name-and-parameters part and its
/// dependency-list part, or `None` if `trimmed` is not a recipe header
/// at all. `nightly := \`sed ...\`` is a variable assignment, not a
/// recipe -- its `:=` is what tells the two apart, since no recipe
/// header can contain that sequence (parameter defaults use `="..."`,
/// never a colon).
fn recipe_header(trimmed: &str) -> Option<(&str, &str)> {
    if trimmed.contains(":=") {
        return None;
    }
    let colon = trimmed.find(':')?;
    Some((&trimmed[..colon], &trimmed[colon + 1..]))
}

/// Parses a justfile into its recipe dependency graph. This is not a
/// general justfile parser -- it understands exactly the constructs
/// this repository's justfile uses (bare recipe headers, parameters
/// with quoted defaults, `#` doc-comments and variable assignments at
/// column zero, indented recipe bodies) and nothing more exotic
/// (recipe attributes, `import`, dependency arguments).
pub(crate) fn parse_justfile(justfile_text: &str) -> JustfileGraph {
    let mut graph = JustfileGraph::default();
    let mut current_recipe: Option<String> = None;
    for line in justfile_text.split('\n') {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(recipe) = &current_recipe {
                graph
                    .edges
                    .entry(recipe.clone())
                    .or_default()
                    .extend(just_recipe_calls(line));
            }
            continue;
        }
        current_recipe = None;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
            continue;
        }
        let Some((header, deps_part)) = recipe_header(trimmed) else {
            continue;
        };
        let Some(name) = header.split_whitespace().next() else {
            continue;
        };
        let deps: BTreeSet<String> = deps_part
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        graph.recipes.insert(name.to_owned());
        graph.edges.entry(name.to_owned()).or_default().extend(deps);
        current_recipe = Some(name.to_owned());
    }
    graph
}

/// Every recipe `root` reaches, directly or transitively, through
/// `edges` -- a `root` the graph has no entry for simply reaches
/// nothing.
pub(crate) fn transitive_closure(
    edges: &BTreeMap<String, BTreeSet<String>>,
    root: &str,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![root.to_owned()];
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(deps) = edges.get(&name) {
            stack.extend(deps.iter().cloned());
        }
    }
    seen
}

/// For every recipe CI actually invokes (deduplicated, first occurrence
/// wins), either it is missing from the justfile entirely or it exists
/// but `ci`/`ci-full` cannot reach it -- reported as two distinct
/// failure shapes, as the property requires.
pub(crate) fn find_violations(
    invocations: &[WorkflowInvocation],
    graph: &JustfileGraph,
) -> Vec<String> {
    let reachable: BTreeSet<String> = transitive_closure(&graph.edges, "ci")
        .into_iter()
        .chain(transitive_closure(&graph.edges, "ci-full"))
        .collect();

    let mut seen_recipes = BTreeSet::new();
    let mut violations = Vec::new();
    for invocation in invocations {
        if !seen_recipes.insert(invocation.recipe.clone()) {
            continue;
        }
        if !graph.recipes.contains(&invocation.recipe) {
            violations.push(format!(
                "`{}` -- invoked at rust.yml:{} (`{}`) but no such recipe exists in the justfile",
                invocation.recipe, invocation.line_number, invocation.line_text
            ));
        } else if !reachable.contains(&invocation.recipe) {
            violations.push(format!(
                "`{}` -- invoked at rust.yml:{} (`{}`) but is not reachable from `ci` or `ci-full`",
                invocation.recipe, invocation.line_number, invocation.line_text
            ));
        }
    }
    violations
}

pub(crate) fn run_at(root: &Path) -> Result<(), String> {
    let workflow_path = root.join(".github/workflows/rust.yml");
    let workflow_text = fs::read_to_string(&workflow_path).map_err(|error| {
        format!(
            "ci-gate-reachability: failed to read {}: {error}",
            workflow_path.display()
        )
    })?;
    let justfile_path = root.join("justfile");
    let justfile_text = fs::read_to_string(&justfile_path).map_err(|error| {
        format!(
            "ci-gate-reachability: failed to read {}: {error}",
            justfile_path.display()
        )
    })?;

    let invocations = parse_workflow_just_invocations(&workflow_text);
    if invocations.is_empty() {
        return Err(format!(
            "ci-gate-reachability: parsed zero `just` invocations out of {} -- the parser \
             broke, or the workflow stopped calling `just` entirely, and either way this \
             gate would silently be checking nothing rather than failing loudly",
            workflow_path.display()
        ));
    }

    let graph = parse_justfile(&justfile_text);
    if graph.recipes.is_empty() {
        return Err(format!(
            "ci-gate-reachability: parsed zero recipes out of {} -- the parser broke",
            justfile_path.display()
        ));
    }

    let violations = find_violations(&invocations, &graph);
    if !violations.is_empty() {
        return Err(super::violations_message(
            format!(
                "ci-gate-reachability: {} recipe(s) CI runs are not reachable from `ci`/`ci-full`:",
                violations.len()
            ),
            &violations,
        ));
    }

    println!(
        "ci-gate-reachability: {} `just` invocation(s) in rust.yml, all reachable from `ci`/`ci-full`",
        invocations.len()
    );
    Ok(())
}
