use std::{collections::BTreeSet, fs, path::Path};

/// Collects the arguments of every `--<flag> <value>` occurrence inside a
/// recipe's body, where the recipe runs from `name:` at column zero to the
/// next line at column zero.
/// The recipe a `just` header declares, ignoring its parameters and
/// dependencies.
///
/// `robot-gpu classes="all":` declares `robot-gpu`. Matching the whole
/// header instead read that as a different recipe and reported the split
/// gone.
fn recipe_header_name(line: &str) -> Option<&str> {
    let (head, _) = line.split_once(':')?;
    head.split_whitespace().next()
}

pub(crate) fn recipe_flag_values(
    justfile_text: &str,
    recipe: &str,
    flag: &str,
) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut inside = false;
    for line in justfile_text.split('\n') {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            if inside {
                break;
            }
            inside = recipe_header_name(line) == Some(recipe);
            continue;
        }
        if !inside {
            continue;
        }
        let mut tokens = line.split_whitespace().peekable();
        while let Some(token) = tokens.next() {
            if token == flag {
                if let Some(value) = tokens.peek() {
                    values.insert((*value).to_owned());
                }
            }
        }
    }
    values
}

pub(crate) fn find_violations(justfile_text: &str) -> Vec<String> {
    let skipped = recipe_flag_values(justfile_text, "robot-gpu", "--skip");
    let captured = recipe_flag_values(justfile_text, "robot-captures", "--example");
    let mut violations = Vec::new();

    // Non-vacuity: two empty sets are trivially equal, and would let this
    // gate pass on a justfile it failed to parse at all.
    if skipped.is_empty() {
        violations.push(
            "`robot-gpu` declares no `--skip` examples; either the split is gone or this \
             gate failed to parse the recipe"
                .to_owned(),
        );
    }
    if captured.is_empty() {
        violations.push(
            "`robot-captures` declares no `--example` entries; either the split is gone or \
             this gate failed to parse the recipe"
                .to_owned(),
        );
    }
    for name in skipped.difference(&captured) {
        violations.push(format!(
            "`{name}` is skipped by `robot-gpu` but not run by `robot-captures`, so it runs \
             nowhere"
        ));
    }
    for name in captured.difference(&skipped) {
        violations.push(format!(
            "`{name}` is run by `robot-captures` but not skipped by `robot-gpu`, so it runs \
             twice"
        ));
    }
    violations
}

pub(crate) fn run_at(root: &Path) -> Result<(), String> {
    let path = root.join("justfile");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("robot-suite-partition: failed to read {path:?}: {error}"))?;
    let violations = find_violations(&text);
    if violations.is_empty() {
        let count = recipe_flag_values(&text, "robot-gpu", "--skip").len();
        println!(
            "robot-suite-partition: the {count} example(s) `robot-gpu` skips are exactly \
             those `robot-captures` runs"
        );
        return Ok(());
    }
    let mut message = format!(
        "robot-suite-partition: {} problem(s) with the robot suite split:",
        violations.len()
    );
    for violation in &violations {
        message.push_str("\n  ");
        message.push_str(violation);
    }
    Err(message)
}
