use std::{collections::BTreeSet, fs, path::Path};

use yaml_rust2::{Yaml, YamlLoader};

const WORKFLOWS: &str = ".github/workflows";
const MAIN_FINISHES: &str = "${{ github.ref != 'refs/heads/main' }}";
const PUBLISH_TAG: &str =
    "${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref_name }}";
const DEFAULT_BRANCH: &str = "${{ github.event.repository.default_branch }}";
const RESOLVED_TAG: &str = "${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.event.workflow_run.head_branch }}";
const FINALIZED_TAG: &str = "${{ needs.create-release.outputs.tag }}";
const NIGHTLY_GATE: &str = "needs.decide.outputs.run == 'true'";
// scripts/ci/pr_budget_test.sh reads this exact condition as "no pull request
// waits for this job".
const NOT_ON_PULL_REQUESTS: &str = "github.event_name != 'pull_request'";
const RELEASE_ASSETS: [&str; 3] = [
    "${{ matrix.artifact_name }}.tar.gz",
    "cranpose-demo-windows-x86_64.zip",
    "apps/android-demo/android/app/build/outputs/apk/release/app-release.apk",
];
const PULL_REQUEST_RECIPES: [&str; 8] = [
    "fmt-check",
    "typos",
    "versions",
    "test",
    "clippy",
    "doc",
    "clippy-wasm",
    "web",
];

type Check = fn(&Workflows, &Path, &mut Vec<String>) -> Result<(), String>;

pub(crate) fn problems(root: &Path) -> Result<Vec<String>, String> {
    let workflows = Workflows::load(&root.join(WORKFLOWS))?;
    let checks: [Check; 11] = [
        overlapping_runs,
        fsmonitor,
        publish_checkout,
        release_trigger,
        release_jobs,
        release_assets,
        nightly_duplicate_check,
        pinned_actions,
        merge_board,
        robot_suite_split,
        provisioning,
    ];
    let mut problems = Vec::new();
    for check in checks {
        check(&workflows, root, &mut problems)?;
    }
    Ok(problems)
}

struct Workflow {
    file: String,
    yaml: Yaml,
}

impl Workflow {
    fn parse(file: String, text: &str) -> Result<Self, String> {
        let mut documents =
            YamlLoader::load_from_str(text).map_err(|error| format!("{file}: {error}"))?;
        if documents.len() != 1 {
            return Err(format!("{file} holds {} YAML documents", documents.len()));
        }
        Ok(Self {
            file,
            yaml: documents.remove(0),
        })
    }

    fn job(&self, name: &str) -> Result<&Yaml, String> {
        let job = &self.yaml["jobs"][name];
        if job.is_badvalue() {
            return Err(format!(
                "{} has no job `{name}`; the checks on it would read nothing",
                self.file
            ));
        }
        Ok(job)
    }

    fn jobs(&self) -> impl Iterator<Item = (&str, &Yaml)> {
        self.yaml["jobs"]
            .as_hash()
            .into_iter()
            .flatten()
            .filter_map(|(name, job)| Some((name.as_str()?, job)))
    }

    fn steps(&self) -> impl Iterator<Item = &Yaml> {
        self.jobs().flat_map(|(_, job)| steps(job))
    }

    fn triggers(&self) -> Vec<&str> {
        match &self.yaml["on"] {
            Yaml::String(trigger) => vec![trigger.as_str()],
            Yaml::Array(triggers) => triggers.iter().filter_map(Yaml::as_str).collect(),
            Yaml::Hash(triggers) => triggers.keys().filter_map(Yaml::as_str).collect(),
            _ => Vec::new(),
        }
    }

    fn runs_recipe(&self, recipe: &[&str]) -> bool {
        self.jobs().any(|(_, job)| runs_recipe(job, recipe))
    }
}

struct Workflows(Vec<Workflow>);

impl Workflows {
    fn load(directory: &Path) -> Result<Self, String> {
        let entries = fs::read_dir(directory)
            .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
        let mut workflows = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file.ends_with(".yml") {
                continue;
            }
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            workflows.push(Workflow::parse(file.to_owned(), &text)?);
        }
        workflows.sort_by(|left, right| left.file.cmp(&right.file));
        Ok(Self(workflows))
    }

    fn named(&self, file: &str) -> Result<&Workflow, String> {
        self.0
            .iter()
            .find(|workflow| workflow.file == file)
            .ok_or_else(|| format!("{WORKFLOWS}/{file} is missing"))
    }
}

fn steps(job: &Yaml) -> impl Iterator<Item = &Yaml> {
    job["steps"].as_vec().into_iter().flatten()
}

fn checkout_refs(job: &Yaml) -> impl Iterator<Item = &str> {
    steps(job)
        .filter(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .filter_map(|step| step["with"]["ref"].as_str())
}

fn runs(job: &Yaml) -> impl Iterator<Item = &str> {
    steps(job).filter_map(|step| step["run"].as_str())
}

// A job runs `recipe` when one of its shell lines is `just` followed by the
// recipe's words, with any further arguments after them.
fn runs_recipe(job: &Yaml, recipe: &[&str]) -> bool {
    runs(job).flat_map(str::lines).any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("just") && recipe.iter().all(|word| words.next() == Some(word))
    })
}

fn needs(job: &Yaml) -> Vec<&str> {
    match &job["needs"] {
        Yaml::String(need) => vec![need.as_str()],
        Yaml::Array(needs) => needs.iter().filter_map(Yaml::as_str).collect(),
        _ => Vec::new(),
    }
}

fn mentions(yaml: &Yaml, needle: &str) -> bool {
    match yaml {
        Yaml::String(text) => text.contains(needle),
        Yaml::Array(items) => items.iter().any(|item| mentions(item, needle)),
        Yaml::Hash(entries) => entries
            .iter()
            .any(|(key, value)| mentions(key, needle) || mentions(value, needle)),
        _ => false,
    }
}

fn strings(yaml: &Yaml) -> Vec<&str> {
    match yaml {
        Yaml::String(text) => vec![text.as_str()],
        Yaml::Array(items) => items.iter().filter_map(Yaml::as_str).collect(),
        _ => Vec::new(),
    }
}

// A pull request's newer push supersedes its older run. On main a run
// finishes: merges come faster than a run takes, and cancelling main's run at
// every merge left main with no answer for a day (#712).
fn overlapping_runs(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let superseded = [
        ("rust.yml", Yaml::String(MAIN_FINISHES.to_owned())),
        (
            "heavy-selfhosted.yml",
            Yaml::String(MAIN_FINISHES.to_owned()),
        ),
        ("build-one.yml", Yaml::Boolean(true)),
    ];
    for (file, cancel) in superseded {
        let concurrency = &workflows.named(file)?.yaml["concurrency"];
        if !concurrency["group"]
            .as_str()
            .is_some_and(|group| group.contains("github.ref"))
        {
            problems.push(format!(
                "{file}: the concurrency group must be per ref, so a newer push supersedes the older run"
            ));
        }
        if concurrency["cancel-in-progress"] != cancel {
            problems.push(format!(
                "{file}: cancel-in-progress must be {cancel:?}: an older pull-request run is dead \
                 weight on the runners, and main needs a finished run"
            ));
        }
    }
    for file in [
        "nightly.yml",
        "publish.yml",
        "release.yml",
        "deploy-pages.yml",
        "cancel-superseded.yml",
    ] {
        let concurrency = &workflows.named(file)?.yaml["concurrency"];
        if concurrency["group"].as_str().is_none_or(str::is_empty) {
            problems.push(format!(
                "{file} must name its concurrency group, so two of its runs cannot overlap"
            ));
        }
        if concurrency["cancel-in-progress"] != Yaml::Boolean(false) {
            problems.push(format!(
                "{file} publishes or deploys; cancelling a run halfway leaves the result half written"
            ));
        }
    }
    Ok(())
}

// The self-hosted Macs' runner user has `core.fsmonitor` on, and
// actions/checkout copies that config: every checkout left a watcher daemon
// behind, and one hung behind a busy daemon for its whole job (nightly
// 38862621908).
fn fsmonitor(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let mut checked = 0;
    for workflow in &workflows.0 {
        let self_hosted = workflow
            .jobs()
            .any(|(_, job)| mentions(&job["runs-on"], "self-hosted"));
        if !self_hosted || workflow.triggers().contains(&"pull_request_target") {
            continue;
        }
        checked += 1;
        let env = &workflow.yaml["env"];
        let disabled = !env["GIT_CONFIG_COUNT"].is_badvalue()
            && env["GIT_CONFIG_KEY_0"].as_str() == Some("core.fsmonitor")
            && env["GIT_CONFIG_VALUE_0"].as_str() == Some("false");
        if !disabled {
            problems.push(format!(
                "{} runs on a self-hosted machine and must set GIT_CONFIG_COUNT, \
                 GIT_CONFIG_KEY_0: core.fsmonitor and GIT_CONFIG_VALUE_0: \"false\" in its env",
                workflow.file
            ));
        }
    }
    if checked == 0 {
        return Err("found no self-hosted workflow; the fsmonitor check has drifted".to_owned());
    }
    Ok(())
}

// sync_versions lands the version bump on the default branch and moves the tag
// onto it. Checking out the branch afterwards published whatever main was:
// for v0.1.139 a pull request that merged in between (#714).
fn publish_checkout(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let publish = workflows.named("publish.yml")?;
    let job = publish.job("publish")?;
    if !checkout_refs(job).any(|reference| reference == PUBLISH_TAG) {
        problems.push(format!(
            "publish.yml: the publish job must check out `{PUBLISH_TAG}`; every release races every merge"
        ));
    }
    if checkout_refs(job).any(|reference| reference == DEFAULT_BRANCH) {
        problems.push("publish.yml: the publish job must not follow the default branch".to_owned());
    }
    // The isolated demo pointer is a commit on the branch, so that job checks
    // the branch out.
    let bump = publish.job("bump_isolated_demo")?;
    if !checkout_refs(bump).any(|reference| reference == DEFAULT_BRANCH) {
        problems.push(
            "publish.yml: bump_isolated_demo commits on the default branch and must check it out"
                .to_owned(),
        );
    }
    Ok(())
}

// A Publish recovered by dispatch carries `main` in workflow_run.head_branch,
// so the board skipped every job and v0.1.139 got no GitHub Release and no
// binaries (#715). Release builds start only after Publish finalized the tag.
fn release_trigger(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let release = workflows.named("release.yml")?;
    let on = &release.yaml["on"];
    if on["workflow_dispatch"]["inputs"]["tag"].is_badvalue() {
        problems.push(
            "release.yml must take a tag by dispatch, or a recovered publish leaves no release"
                .to_owned(),
        );
    }
    if strings(&on["workflow_run"]["workflows"]) != ["Publish"]
        || strings(&on["workflow_run"]["types"]) != ["completed"]
        || !on["push"].is_badvalue()
    {
        problems.push(
            "release.yml must start only from a completed Publish run, which finalizes the tag"
                .to_owned(),
        );
    }

    let create = release.job("create-release")?;
    let condition = create["if"].as_str().unwrap_or_default();
    if !condition.contains("github.event.workflow_run.conclusion == 'success'")
        || !condition.contains("startsWith(github.event.workflow_run.head_branch, 'v')")
    {
        problems.push(
            "release.yml: create-release must skip a failed Publish and one that did not run from a tag"
                .to_owned(),
        );
    }
    if create["outputs"]["tag"].as_str() != Some(RESOLVED_TAG) {
        problems.push(format!(
            "release.yml: create-release must output the resolved tag `{RESOLVED_TAG}`"
        ));
    }
    if !runs(create).any(|run| run.contains(r"^v[0-9]+\.[0-9]+\.[0-9]+$")) {
        problems.push(
            "release.yml: create-release must check a dispatched tag's shape before it opens a release"
                .to_owned(),
        );
    }
    Ok(())
}

// Every job below create-release reads the one tag it resolved.
fn release_jobs(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let release = workflows.named("release.yml")?;
    for (name, job) in release.jobs().filter(|(name, _)| *name != "create-release") {
        if !needs(job).contains(&"create-release") {
            problems.push(format!(
                "release.yml: {name} must need create-release to read its tag"
            ));
        }
        if mentions(job, "github.event.workflow_run.head_branch") {
            problems.push(format!(
                "release.yml: {name} must take the tag from create-release, not from the event"
            ));
        }
        let mut checkouts = checkout_refs(job).peekable();
        if checkouts.peek().is_none() || checkouts.any(|reference| reference != FINALIZED_TAG) {
            problems.push(format!(
                "release.yml: {name} must check out the finalized tag `{FINALIZED_TAG}`"
            ));
        }
    }
    for (name, job) in release.jobs() {
        let expected = if name == "create-release" {
            RESOLVED_TAG
        } else {
            FINALIZED_TAG
        };
        for step in steps(job) {
            let tag = &step["with"]["tag_name"];
            if !tag.is_badvalue() && tag.as_str() != Some(expected) {
                problems.push(format!(
                    "release.yml: a release step in {name} must target `{expected}`"
                ));
            }
        }
    }
    if mentions(&release.yaml, "github.ref_name") {
        problems.push(
            "release.yml must not read github.ref_name: on workflow_run it names the default branch"
                .to_owned(),
        );
    }
    Ok(())
}

fn release_assets(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let release = workflows.named("release.yml")?;
    let mut uploaded = BTreeSet::new();
    for step in release.steps() {
        let Some(files) = step["with"]["files"].as_str() else {
            continue;
        };
        if files.contains('*') {
            problems.push(format!(
                "release.yml uploads `{files}`; a glob can silently match no file"
            ));
        }
        if step["with"]["fail_on_unmatched_files"] != Yaml::Boolean(true) {
            problems.push(format!(
                "release.yml: the upload of `{files}` must set fail_on_unmatched_files: true"
            ));
        }
        uploaded.insert(files);
    }
    for asset in RELEASE_ASSETS {
        if !uploaded.contains(asset) {
            problems.push(format!("release.yml no longer uploads `{asset}`"));
        }
    }
    Ok(())
}

// The nightly board has two triggers: GitHub's schedule, which once came late
// and then never, and a crontab on samarch-1. Every job waits on `decide`, or
// two triggers run the suite twice (#716).
fn nightly_duplicate_check(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let nightly = workflows.named("nightly.yml")?;
    if nightly.yaml["on"]["workflow_dispatch"]["inputs"]["force"].is_badvalue() {
        problems.push(
            "nightly.yml must take a `force` input, so a person can run a night that is covered"
                .to_owned(),
        );
    }
    let decide = nightly.job("decide")?;
    if decide["runs-on"].as_str() != Some("ubuntu-latest") {
        problems.push(
            "nightly.yml: decide must run on ubuntu-latest, not on a self-hosted runner".to_owned(),
        );
    }
    if decide["outputs"]["run"].as_str() != Some("${{ steps.check.outputs.run }}")
        || !runs(decide).any(|run| run.contains("scripts/ci/nightly_should_run.sh"))
    {
        problems.push(
            "nightly.yml: decide must publish the answer of scripts/ci/nightly_should_run.sh"
                .to_owned(),
        );
    }
    for (name, job) in nightly.jobs().filter(|(name, _)| *name != "decide") {
        let gated = needs(job).contains(&"decide")
            && job["if"]
                .as_str()
                .is_some_and(|condition| condition.contains(NIGHTLY_GATE));
        if !gated {
            problems.push(format!(
                "nightly.yml: {name} must need decide and run only if `{NIGHTLY_GATE}`"
            ));
        }
    }
    Ok(())
}

fn pinned_actions(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let mut seen = 0;
    for workflow in &workflows.0 {
        let references = workflow
            .jobs()
            .filter_map(|(_, job)| job["uses"].as_str())
            .chain(workflow.steps().filter_map(|step| step["uses"].as_str()));
        for reference in references {
            seen += 1;
            if reference.starts_with("./") {
                continue;
            }
            let pinned = reference.split_once('@').is_some_and(|(_, git_ref)| {
                git_ref.len() == 40 && git_ref.chars().all(|c| c.is_ascii_hexdigit())
            });
            if !pinned {
                problems.push(format!(
                    "{}: `{reference}` must be pinned to a 40-character commit SHA",
                    workflow.file
                ));
            }
        }
    }
    if seen < 10 {
        return Err(format!(
            "found only {seen} action references; the pinned-action check has drifted"
        ));
    }
    Ok(())
}

// Short checks of the tree that just landed run on the board a merge
// triggers; the pull-request board and the nightly do not repeat them.
fn merge_board(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let pull_request = workflows.named("rust.yml")?;
    let heavy = workflows.named("heavy-selfhosted.yml")?;
    let nightly = workflows.named("nightly.yml")?;
    for job in ["architecture-budget", "android-apk"] {
        if heavy.job(job)?["if"].as_str() != Some(NOT_ON_PULL_REQUESTS) {
            problems.push(format!(
                "heavy-selfhosted.yml: {job} must not run on a pull request (`if: {NOT_ON_PULL_REQUESTS}`)"
            ));
        }
        if nightly.job(job).is_ok() {
            problems.push(format!(
                "nightly.yml runs {job}, which the merge board already runs"
            ));
        }
    }
    for recipe in PULL_REQUEST_RECIPES {
        if !pull_request.runs_recipe(&[recipe]) {
            problems.push(format!("rust.yml no longer runs `just {recipe}`"));
        }
    }
    if pull_request.runs_recipe(&["budgets"]) {
        problems.push("rust.yml runs `just budgets`; no pull request waits for it".to_owned());
    }
    for recipe in ["budgets", "android"] {
        if !heavy.runs_recipe(&[recipe]) {
            problems.push(format!(
                "heavy-selfhosted.yml no longer runs `just {recipe}`"
            ));
        }
    }
    Ok(())
}

// Every push to main runs the parallel robot class; the nightly runs the
// measuring half alone, one example at a time on an emptied machine.
fn robot_suite_split(
    workflows: &Workflows,
    _root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let heavy = workflows.named("heavy-selfhosted.yml")?;
    let nightly = workflows.named("nightly.yml")?;
    if !nightly.runs_recipe(&["robot-linux", "serial"]) {
        problems.push("nightly.yml no longer runs `just robot-linux serial`".to_owned());
    }
    let whole_suite = |workflow: &Workflow| {
        workflow
            .jobs()
            .flat_map(|(_, job)| runs(job))
            .flat_map(str::lines)
            .any(|line| {
                let words: Vec<&str> = line.split_whitespace().collect();
                words == ["just", "robot-linux"]
                    || words.starts_with(&["just", "robot-linux", "all"])
            })
    };
    if whole_suite(nightly) || whole_suite(heavy) {
        problems.push(
            "the whole robot suite runs again: main runs its parallel class and the nightly its serial class"
                .to_owned(),
        );
    }
    Ok(())
}

fn provisioning(
    workflows: &Workflows,
    root: &Path,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let pull_request = workflows.named("rust.yml")?;
    let heavy = workflows.named("heavy-selfhosted.yml")?;
    let release = workflows.named("release.yml")?;
    if !pull_request
        .jobs()
        .any(|(_, job)| runs(job).any(|run| run == "scripts/ci/provision_toolchain.sh"))
    {
        problems.push("rust.yml must provision the toolchain before its recipes".to_owned());
    }
    // The broad action installs SDK packages no job uses.
    for workflow in [pull_request, release] {
        let broad = workflow.steps().any(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("android-actions/setup-android@"))
        });
        if broad {
            problems.push(format!(
                "{} runs android-actions/setup-android; install the required SDK packages alone",
                workflow.file
            ));
        }
    }
    ndk_version(root, heavy, release, problems)
}

fn ndk_version(
    root: &Path,
    heavy: &Workflow,
    release: &Workflow,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let android_env = root.join("scripts/ci/android_env.sh");
    let text = fs::read_to_string(&android_env)
        .map_err(|error| format!("cannot read {}: {error}", android_env.display()))?;
    let version = text
        .split("/ndk/")
        .nth(1)
        .and_then(|rest| {
            rest.split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .next()
        })
        .filter(|version| !version.is_empty())
        .ok_or_else(|| "scripts/ci/android_env.sh names no NDK version".to_owned())?;
    let heavy_installs = heavy
        .jobs()
        .flat_map(|(_, job)| runs(job))
        .any(|run| run.contains(&format!("sdkmanager \"ndk;{version}\"")));
    let release_installs = release
        .jobs()
        .flat_map(|(_, job)| runs(job))
        .any(|run| run.contains(&format!("install_android_ndk.sh {version}")));
    if !heavy_installs || !release_installs {
        problems.push(format!(
            "scripts/ci/android_env.sh points at NDK {version}; heavy-selfhosted.yml and release.yml \
             must install that version"
        ));
    }
    Ok(())
}
