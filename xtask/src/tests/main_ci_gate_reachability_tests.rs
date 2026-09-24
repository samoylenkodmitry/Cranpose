use crate::ci_gate_reachability::{
    find_violations, parse_justfile, parse_workflow_just_invocations, transitive_closure,
};

const JUSTFILE: &str = "\
# a doc comment mentioning `just orphaned` must not create an edge
alpha:
    cargo alpha

beta:
    cargo beta

deep:
    cargo deep

middle: deep
    cargo middle

platform-only:
    cargo platform

takes-args target=\"x\":
    cargo run {{target}}

body-caller:
    just beta

ci: alpha middle body-caller takes-args
ci-full: ci platform-only
";

fn workflow(body: &str) -> String {
    format!("jobs:\n  check:\n    steps:\n{body}")
}

#[test]
fn a_recipe_reachable_only_through_ci_is_not_a_violation() {
    let calls = parse_workflow_just_invocations(&workflow("      - run: just alpha\n"));
    assert_eq!(calls.len(), 1, "the workflow declares exactly one call");
    assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
}

#[test]
fn a_recipe_reachable_only_through_ci_full_is_not_a_violation() {
    let calls = parse_workflow_just_invocations(&workflow("      - run: just platform-only\n"));
    assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
}

#[test]
fn a_recipe_reached_through_a_dependency_chain_is_not_a_violation() {
    // ci -> middle -> deep. Only the transitive step makes `deep` reachable.
    let graph = parse_justfile(JUSTFILE);
    assert!(transitive_closure(&graph.edges, "ci").contains("deep"));
    let calls = parse_workflow_just_invocations(&workflow("      - run: just deep\n"));
    assert!(find_violations(&calls, &graph).is_empty());
}

#[test]
fn a_recipe_a_body_shells_out_to_is_reachable() {
    // `body-caller` runs `just beta` from its body rather than declaring
    // it. Reading only recipe headers would report `beta` unreachable.
    let calls = parse_workflow_just_invocations(&workflow("      - run: just beta\n"));
    assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
}

#[test]
fn a_recipe_in_neither_aggregate_is_a_violation() {
    let calls = parse_workflow_just_invocations(&workflow("      - run: just orphaned\n"));
    let violations = find_violations(&calls, &parse_justfile(JUSTFILE));
    assert_eq!(violations.len(), 1, "got {violations:?}");
    assert!(violations[0].contains("orphaned"), "got {violations:?}");
}

#[test]
fn a_recipe_absent_from_the_justfile_reports_distinctly() {
    let defined = parse_workflow_just_invocations(&workflow("      - run: just beta\n"));
    let mut undefined = parse_workflow_just_invocations(&workflow("      - run: just nope\n"));
    undefined.extend(defined);
    let violations = find_violations(&undefined, &parse_justfile(JUSTFILE));
    assert_eq!(
        violations.len(),
        1,
        "only the undefined one fails: {violations:?}"
    );
    assert!(
        violations[0].contains("no such recipe exists"),
        "an undefined recipe must not read as merely unreachable: {violations:?}"
    );
}

#[test]
fn a_recipe_invoked_with_arguments_is_matched_by_name() {
    let calls = parse_workflow_just_invocations(&workflow("      - run: just takes-args value\n"));
    assert_eq!(calls[0].recipe, "takes-args");
    assert!(find_violations(&calls, &parse_justfile(JUSTFILE)).is_empty());
}

#[test]
fn calls_inside_a_block_scalar_are_found_and_prose_is_not() {
    let text = workflow(
        "      # just orphaned in a comment is prose, not a call\n\
         \x20     - run: |\n\
         \x20         just alpha\n\
         \x20\n\
         \x20         just beta\n\
         \x20     - name: mentions `just orphaned` in prose\n",
    );
    let found: Vec<String> = parse_workflow_just_invocations(&text)
        .into_iter()
        .map(|call| call.recipe)
        .collect();
    assert_eq!(
        found,
        vec!["alpha".to_owned(), "beta".to_owned()],
        "got {found:?}"
    );
}

#[test]
fn the_real_workflow_parses_to_a_non_zero_number_of_calls() {
    // Without this the whole gate is vacuous: a parser that silently
    // finds nothing reports every repository clean.
    let root = crate::workspace_root().expect("workspace root");
    let text = std::fs::read_to_string(root.join(".github/workflows/rust.yml"))
        .expect("rust.yml is readable");
    let calls = parse_workflow_just_invocations(&text);
    assert!(
        calls.len() >= 10,
        "rust.yml must parse to a meaningful number of `just` calls, got {}",
        calls.len()
    );
}
