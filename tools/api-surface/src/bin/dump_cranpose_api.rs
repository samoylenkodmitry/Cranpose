use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use cranpose_api_surface::resolve::{Loader, MemberItem, compute_reachability};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct MemberOut {
    name: String,
    signature: String,
}

impl MemberOut {
    fn from_reachable(m: &MemberItem) -> Option<Self> {
        m.reachable.get().then(|| MemberOut {
            name: m.name.clone(),
            signature: m.signature.clone(),
        })
    }
}

#[derive(Serialize)]
struct ItemOut {
    module_path: String,
    ident: String,
    kind: String,
    composable: bool,
    signature: String,
    members: Vec<MemberOut>,
}

#[derive(Serialize)]
struct CrateOut {
    crate_name: String,
    manifest_path: String,
    items: Vec<ItemOut>,
    warnings: Vec<String>,
}

struct WorkspacePackage {
    name: String,
    lib_src_path: PathBuf,
    manifest_path: PathBuf,
}

fn cargo_metadata_json(workspace_root: &Path) -> Result<Value> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace_root)
        .output()
        .context("failed to run cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("cargo metadata did not return valid JSON")
}

fn is_publishable(pkg: &Value) -> bool {
    match pkg.get("publish") {
        None | Some(Value::Null) => true,
        Some(Value::Array(registries)) => !registries.is_empty(),
        _ => true,
    }
}

fn is_library_crate(manifest_path: &Path, workspace_root: &Path) -> bool {
    manifest_path
        .strip_prefix(workspace_root)
        .map(|rel| rel.starts_with("crates"))
        .unwrap_or(false)
}

fn discover_workspace_packages(
    metadata: &Value,
    workspace_root: &Path,
) -> Result<Vec<WorkspacePackage>> {
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .context("cargo metadata missing `packages`")?;
    let mut out = vec![];
    for pkg in packages {
        if !is_publishable(pkg) {
            continue;
        }
        let manifest_path_str = pkg
            .get("manifest_path")
            .and_then(Value::as_str)
            .context("package missing `manifest_path`")?;
        if !is_library_crate(Path::new(manifest_path_str), workspace_root) {
            continue;
        }
        let name = pkg
            .get("name")
            .and_then(Value::as_str)
            .context("package missing `name`")?
            .to_string();
        let manifest_path = pkg
            .get("manifest_path")
            .and_then(Value::as_str)
            .context("package missing `manifest_path`")?;
        let manifest_path = PathBuf::from(manifest_path);
        let Some(targets) = pkg.get("targets").and_then(Value::as_array) else {
            continue;
        };
        let lib_target = targets.iter().find(|t| {
            t.get("kind")
                .and_then(Value::as_array)
                .is_some_and(|kinds| {
                    kinds.iter().any(|k| {
                        matches!(k.as_str(), Some("lib") | Some("rlib") | Some("proc-macro"))
                    })
                })
        });
        let Some(lib_target) = lib_target else {
            continue;
        };
        let src_path = lib_target
            .get("src_path")
            .and_then(Value::as_str)
            .context("lib target missing `src_path`")?;
        out.push(WorkspacePackage {
            name,
            lib_src_path: PathBuf::from(src_path),
            manifest_path,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn dump_crate(pkg: &WorkspacePackage) -> Result<CrateOut> {
    let mut loader = Loader::new();
    loader.load_crate(&pkg.lib_src_path).with_context(|| {
        format!(
            "loading crate {} at {}",
            pkg.name,
            pkg.lib_src_path.display()
        )
    })?;
    compute_reachability(&loader.tree);

    let mut items = vec![];
    for it in &loader.tree.named_items {
        if !it.reachable.get() {
            continue;
        }
        let members: Vec<MemberOut> = it
            .sub_items
            .iter()
            .filter_map(MemberOut::from_reachable)
            .collect();
        items.push(ItemOut {
            module_path: it.module_path.join("::"),
            ident: it.ident.clone(),
            kind: it.kind.clone(),
            composable: it.composable,
            signature: it.signature.clone(),
            members,
        });
    }
    for imp in &loader.tree.deferred_impls {
        if !imp.reachable.get() {
            continue;
        }
        let self_ty = imp.self_type_ident.clone().unwrap_or_default();
        for m in &imp.members {
            if let Some(mo) = MemberOut::from_reachable(m) {
                items.push(ItemOut {
                    module_path: imp.module_path.join("::"),
                    ident: format!("{self_ty}::{}", mo.name),
                    kind: "impl_fn".to_string(),
                    composable: false,
                    signature: mo.signature,
                    members: vec![],
                });
            }
        }
    }
    for (path, node) in loader.tree.modules.iter() {
        if !node.reachable.get() {
            continue;
        }
        for u in &node.use_items {
            if u.reachable.get() {
                items.push(ItemOut {
                    module_path: path.join("::"),
                    ident: format!("use {}", u.signature),
                    kind: "reexport".to_string(),
                    composable: false,
                    signature: u.signature.clone(),
                    members: vec![],
                });
            }
        }
    }

    Ok(CrateOut {
        crate_name: pkg.name.clone(),
        manifest_path: pkg.manifest_path.display().to_string(),
        items,
        warnings: loader.tree.warnings.clone(),
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut out_path: Option<PathBuf> = None;
    let mut workspace_root: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out_path = Some(PathBuf::from(args.next().context("--out needs a value")?)),
            "--workspace-root" => {
                workspace_root = Some(PathBuf::from(
                    args.next().context("--workspace-root needs a value")?,
                ))
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    let out_path = out_path.context("--out is required")?;
    let workspace_root = workspace_root.unwrap_or_else(|| PathBuf::from("."));

    let metadata = cargo_metadata_json(&workspace_root)?;
    let metadata_workspace_root = metadata
        .get("workspace_root")
        .and_then(Value::as_str)
        .context("cargo metadata missing `workspace_root`")?;
    let packages = discover_workspace_packages(&metadata, Path::new(metadata_workspace_root))?;
    eprintln!("discovered {} publishable library crates", packages.len());

    let mut all = vec![];
    for pkg in &packages {
        all.push(dump_crate(pkg)?);
    }
    let json = serde_json::to_string_pretty(&all)?;
    std::fs::write(&out_path, &json).with_context(|| format!("writing {}", out_path.display()))?;
    println!("wrote {} crates -> {}", all.len(), out_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn publish_null_is_publishable() {
        assert!(is_publishable(&json!({"publish": null})));
    }

    #[test]
    fn publish_missing_key_is_publishable() {
        assert!(is_publishable(&json!({})));
    }

    #[test]
    fn publish_empty_array_is_not_publishable() {
        assert!(!is_publishable(&json!({"publish": []})));
    }

    #[test]
    fn publish_restricted_registry_is_publishable() {
        assert!(is_publishable(&json!({"publish": ["internal"]})));
    }

    #[test]
    fn crate_under_crates_dir_is_a_library_crate() {
        let root = Path::new("/repo");
        assert!(is_library_crate(
            Path::new("/repo/crates/cranpose-core/Cargo.toml"),
            root
        ));
    }

    #[test]
    fn crate_under_apps_dir_is_not_a_library_crate() {
        let root = Path::new("/repo");
        assert!(!is_library_crate(
            Path::new("/repo/apps/desktop-demo/Cargo.toml"),
            root
        ));
    }

    #[test]
    fn proc_macro_target_kind_is_discovered() {
        let metadata = json!({
            "workspace_root": "/repo",
            "packages": [{
                "name": "cranpose-macros",
                "publish": null,
                "manifest_path": "/repo/crates/cranpose-macros/Cargo.toml",
                "targets": [{
                    "kind": ["proc-macro"],
                    "src_path": "/repo/crates/cranpose-macros/src/lib.rs",
                }],
            }],
        });
        let packages = discover_workspace_packages(&metadata, Path::new("/repo")).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "cranpose-macros");
    }
}
