use super::*;

#[test]
fn parse_bundle_defaults() {
    let options = BundleMacosOptions::parse(&[]).expect("default options parse");

    assert_eq!(options.package, "desktop-app");
    assert_eq!(options.bin, "desktop-app");
    assert_eq!(options.profile, "release");
    assert_eq!(options.app_name, "Cranpose Demo");
    assert_eq!(options.bundle_id, "io.cranpose.demo");
    assert!(options.build);
}

#[test]
fn parse_bundle_options() {
    let options = BundleMacosOptions::parse(&[
        "--package".into(),
        "isolated-demo".into(),
        "--bin".into(),
        "isolated-demo".into(),
        "--profile".into(),
        "release-small".into(),
        "--app-name".into(),
        "Cranpose Isolated".into(),
        "--bundle-id".into(),
        "io.cranpose.isolated".into(),
        "--out-dir".into(),
        "target/custom-bundles".into(),
        "--target".into(),
        "aarch64-apple-darwin".into(),
        "--no-build".into(),
        "--sign-identity".into(),
        "Developer ID Application: Example".into(),
    ])
    .expect("custom options parse");

    assert_eq!(options.package, "isolated-demo");
    assert_eq!(options.profile, "release-small");
    assert_eq!(options.target.as_deref(), Some("aarch64-apple-darwin"));
    assert!(!options.build);
    assert_eq!(
        options.sign_identity.as_deref(),
        Some("Developer ID Application: Example")
    );
}

#[test]
fn parse_binary_size_options() {
    let options = BinarySizeOptions::parse(&[
        "--package".into(),
        "desktop-app".into(),
        "--bin".into(),
        "desktop-app".into(),
        "--profile".into(),
        "dev".into(),
        "--target".into(),
        "x86_64-unknown-linux-gnu".into(),
        "--manifest-path".into(),
        "apps/isolated-demo/Cargo.toml".into(),
        "--max-bytes".into(),
        "29360128".into(),
        "--patch-workspace-cranpose".into(),
        "--no-build".into(),
    ])
    .expect("binary-size options parse");

    assert_eq!(options.binary.package, "desktop-app");
    assert_eq!(options.binary.profile, "dev");
    assert_eq!(
        options.binary.target.as_deref(),
        Some("x86_64-unknown-linux-gnu")
    );
    assert_eq!(
        options.binary.manifest_path.as_deref(),
        Some(Path::new("apps/isolated-demo/Cargo.toml"))
    );
    assert_eq!(options.max_bytes, Some(29_360_128));
    assert!(options.binary.patch_workspace_cranpose);
    assert!(!options.build);
}

#[test]
fn parse_dist_min_feature_flags_and_target_rustflags() {
    let options = DistMinOptions::parse(&[
        "--features".into(),
        "desktop,renderer-wgpu".into(),
        "--no-default-features".into(),
    ])
    .expect("dist-min feature options parse");
    assert_eq!(options.features.as_deref(), Some("desktop,renderer-wgpu"));
    assert!(options.no_default_features);

    let linux = dist_min_rustflags_for_target("x86_64-unknown-linux-gnu");
    assert!(linux.contains("--icf=all"), "linux gets lld icf: {linux}");
    let android = dist_min_rustflags_for_target("aarch64-linux-android");
    assert!(android.contains("--icf=all"), "android gets lld icf");
    let mac = dist_min_rustflags_for_target("aarch64-apple-darwin");
    assert!(!mac.contains("lld"), "ld64 targets skip lld flags: {mac}");
    let win = dist_min_rustflags_for_target("x86_64-pc-windows-msvc");
    assert!(!win.contains("icf"), "msvc has /OPT:ICF already: {win}");
    assert!(
        !win.contains("force-unwind-tables"),
        "msvc requires unwind tables: {win}"
    );
    assert!(linux.contains("-Cforce-unwind-tables=no"));
}

#[test]
fn parse_dist_min_options() {
    let options = DistMinOptions::parse(&[
        "--package".into(),
        "isolated-demo".into(),
        "--bin".into(),
        "isolated-demo".into(),
        "--target".into(),
        "x86_64-unknown-linux-gnu".into(),
        "--manifest-path".into(),
        "apps/isolated-demo/Cargo.toml".into(),
        "--max-bytes".into(),
        "6291456".into(),
        "--patch-workspace-cranpose".into(),
    ])
    .expect("dist-min options parse");

    assert_eq!(options.binary.package, "isolated-demo");
    assert_eq!(options.binary.bin, "isolated-demo");
    assert_eq!(options.binary.profile, "release-small");
    assert_eq!(
        options.binary.target.as_deref(),
        Some("x86_64-unknown-linux-gnu")
    );
    assert_eq!(
        options.binary.manifest_path.as_deref(),
        Some(Path::new("apps/isolated-demo/Cargo.toml"))
    );
    assert_eq!(options.max_bytes, Some(6_291_456));
    assert!(options.binary.patch_workspace_cranpose);
}

#[test]
fn parse_dist_min_rejects_unknown_option() {
    let error = DistMinOptions::parse(&["--no-build".into()])
        .expect_err("dist-min should reject binary-size-only flags");
    assert!(error.contains("--no-build"));
}

#[test]
fn parse_binary_size_rejects_invalid_budget() {
    let error = BinarySizeOptions::parse(&["--max-bytes".into(), "not-a-number".into()])
        .expect_err("invalid max bytes should fail");

    assert!(error.contains("--max-bytes must be an unsigned integer"));
}

#[test]
fn parse_dependency_budget_defaults_to_workspace_and_all_features() {
    let options = DependencyBudgetOptions::parse(&[]).expect("dependency-budget options parse");

    assert_eq!(
        options.scopes,
        vec![
            DependencyBudgetScope::Workspace,
            DependencyBudgetScope::AllFeatures
        ]
    );
    assert!(!options.explain);
}

#[test]
fn parse_dependency_budget_accepts_single_scope_options() {
    let workspace = DependencyBudgetOptions::parse(&["--workspace-only".into()])
        .expect("workspace-only options parse");
    assert_eq!(workspace.scopes, vec![DependencyBudgetScope::Workspace]);
    assert!(!workspace.explain);

    let all_features = DependencyBudgetOptions::parse(&["--all-features-only".into()])
        .expect("all-features-only options parse");
    assert_eq!(
        all_features.scopes,
        vec![DependencyBudgetScope::AllFeatures]
    );
    assert!(!all_features.explain);
}

#[test]
fn parse_dependency_budget_accepts_explain_with_scope() {
    let options = DependencyBudgetOptions::parse(&["--workspace-only".into(), "--explain".into()])
        .expect("dependency-budget explain options parse");

    assert_eq!(options.scopes, vec![DependencyBudgetScope::Workspace]);
    assert!(options.explain);
}

#[test]
fn parse_dependency_budget_rejects_unknown_option() {
    let error = DependencyBudgetOptions::parse(&["--unexpected".into()])
        .expect_err("unknown option should fail");

    assert!(error.contains("unknown dependency-budget option"));
}

#[test]
fn plist_escapes_bundle_fields() {
    let plist = info_plist("Cranpose & Demo", "Cranpose<Demo>", "io.cranpose.demo");

    assert!(plist.contains("Cranpose &amp; Demo"));
    assert!(plist.contains("Cranpose&lt;Demo&gt;"));
    assert!(plist.contains("io.cranpose.demo"));
}

#[test]
fn duplicate_budget_parser_returns_root_package_families() {
    let tree = "\
hashbrown v0.15.5
└── gpu-descriptor v0.3.2

hashbrown v0.16.1
└── naga v29.0.3

tiny-skia v0.12.0 (*)
";

    let details = duplicate_package_details(tree);
    assert_eq!(
        duplicate_version_package_families(&details),
        vec!["hashbrown".to_owned()]
    );
}

#[test]
fn dependency_budget_cargo_tree_args_pin_shipped_targets() {
    let shipped_targets = [
        "aarch64-apple-darwin",
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "aarch64-linux-android",
        "armv7-linux-androideabi",
        "i686-linux-android",
        "wasm32-unknown-unknown",
        "x86_64-linux-android",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
    ];

    for scope in [
        DependencyBudgetScope::Workspace,
        DependencyBudgetScope::AllFeatures,
    ] {
        let args = scope.cargo_tree_args();
        for target in shipped_targets {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == "--target" && pair[1] == target),
                "budget cargo tree args for {} must pin --target {target}",
                scope.label()
            );
        }
    }
}

#[test]
fn duplicate_budget_parser_ignores_coloured_nested_lines() {
    let tree = concat!(
        "\u{1b}[2m│\u{1b}[0m   \u{1b}[2m└──\u{1b}[0m thiserror v1.0.69\n",
        "\u{1b}[2m│\u{1b}[0m   \u{1b}[2m└──\u{1b}[0m thiserror v2.0.18\n",
    );

    let details = duplicate_package_details(tree);

    assert!(
        duplicate_version_package_families(&details).is_empty(),
        "nested tree lines are dependents, not roots, coloured or not"
    );
}

#[test]
fn duplicate_budget_parser_reads_coloured_root_lines() {
    let tree = concat!(
        "\u{1b}[1mthiserror\u{1b}[0m v1.0.69\n",
        "\u{1b}[2m└──\u{1b}[0m ndk v0.9.0\n",
        "\n",
        "\u{1b}[1mthiserror\u{1b}[0m v2.0.18\n",
        "\u{1b}[2m└──\u{1b}[0m cranpose v0.1.101\n",
    );

    let details = duplicate_package_details(tree);

    assert_eq!(
        duplicate_version_package_families(&details),
        vec!["thiserror".to_owned()],
        "a coloured root line names the same family as an uncoloured one"
    );
}

#[test]
fn cargo_tree_package_parser_strips_colour() {
    let tree = "\u{1b}[1mcranpose-render-pixels\u{1b}[0m v0.1.101\n";

    assert_eq!(
        package_names_in_cargo_tree(tree),
        vec!["cranpose-render-pixels".to_owned()]
    );
}

#[test]
fn duplicate_budget_violation_rejects_unrecorded_families() {
    let families = vec!["foldhash".to_owned(), "hashbrown".to_owned()];

    let violation = duplicate_budget_violation(DependencyBudgetScope::Workspace, &families, &[])
        .expect("unrecorded duplicate-version families should fail the budget");

    assert!(violation.contains(
        "unexpected duplicate dependency version families for workspace: foldhash, hashbrown;"
    ));
}

#[test]
fn duplicate_budget_violation_rejects_stale_recorded_debt() {
    let debt = DuplicateDebt {
        family: "hashbrown",
        reason: "an upstream split that no longer exists",
    };

    let violation = duplicate_budget_violation(DependencyBudgetScope::Workspace, &[], &[&debt])
        .expect("stale recorded debt should fail the budget");

    assert!(violation.contains("stale duplicate dependency debt for workspace: hashbrown;"));
}

#[test]
fn duplicate_budget_violation_reports_unexpected_and_stale_together() {
    let debt = DuplicateDebt {
        family: "hashbrown",
        reason: "an upstream split that no longer exists",
    };
    let families = vec!["new-family".to_owned()];

    let violation =
        duplicate_budget_violation(DependencyBudgetScope::Workspace, &families, &[&debt])
            .expect("unexpected and stale families should both fail the budget");

    assert!(violation.contains("unexpected duplicate dependency version families"));
    assert!(violation.contains("new-family"));
    assert!(violation.contains("stale duplicate dependency debt"));
    assert!(violation.contains("hashbrown"));
}

#[test]
fn duplicate_budget_violation_accepts_exact_debt_match() {
    let debt = DuplicateDebt {
        family: "hashbrown",
        reason: "an upstream pin",
    };
    let families = vec!["hashbrown".to_owned()];

    assert_eq!(
        duplicate_budget_violation(DependencyBudgetScope::Workspace, &families, &[&debt]),
        None
    );
}

#[test]
fn duplicate_budget_violation_accepts_empty_tree_and_empty_debt() {
    assert_eq!(
        duplicate_budget_violation(DependencyBudgetScope::Workspace, &[], &[]),
        None
    );
}

#[test]
fn recorded_debt_families_are_unique_per_scope() {
    for scope in [
        DependencyBudgetScope::Workspace,
        DependencyBudgetScope::AllFeatures,
    ] {
        let mut families = scope
            .recorded_debt()
            .iter()
            .map(|debt| debt.family)
            .collect::<Vec<_>>();
        families.sort_unstable();
        let mut deduped = families.clone();
        deduped.dedup();
        assert_eq!(families, deduped, "duplicate debt entry for {scope:?}");
    }
}

#[test]
fn dependency_budget_result_aggregates_scope_errors() {
    let error = dependency_budget_result(vec![
        "unexpected duplicate dependency version families for workspace: hashbrown".to_owned(),
        "unexpected duplicate dependency version families for workspace all-features: roxmltree"
            .to_owned(),
    ])
    .expect_err("scope errors should be aggregated");

    assert!(error.contains("workspace: hashbrown"));
    assert!(error.contains("workspace all-features: roxmltree"));
    assert_eq!(error.lines().count(), 2);
}

#[test]
fn dependency_budget_result_accepts_no_scope_errors() {
    assert_eq!(dependency_budget_result(Vec::new()), Ok(()));
}

#[test]
fn duplicate_details_printer_accepts_empty_focused_family_list() {
    print_duplicate_package_details(DependencyBudgetScope::Workspace, &[], &[]);
}

#[test]
fn duplicate_version_families_ignore_repeated_same_version_roots() {
    let details = vec![
        DuplicatePackageFamily {
            name: "hashbrown".to_owned(),
            roots: vec![
                DuplicatePackageRoot {
                    name: "hashbrown".to_owned(),
                    version: "0.15.5".to_owned(),
                    root: "hashbrown v0.15.5".to_owned(),
                    direct_dependents: Vec::new(),
                },
                DuplicatePackageRoot {
                    name: "hashbrown".to_owned(),
                    version: "0.16.1".to_owned(),
                    root: "hashbrown v0.16.1".to_owned(),
                    direct_dependents: Vec::new(),
                },
            ],
        },
        DuplicatePackageFamily {
            name: "serde".to_owned(),
            roots: vec![
                DuplicatePackageRoot {
                    name: "serde".to_owned(),
                    version: "1.0.228".to_owned(),
                    root: "serde v1.0.228".to_owned(),
                    direct_dependents: Vec::new(),
                },
                DuplicatePackageRoot {
                    name: "serde".to_owned(),
                    version: "1.0.228".to_owned(),
                    root: "serde v1.0.228".to_owned(),
                    direct_dependents: Vec::new(),
                },
            ],
        },
    ];

    assert_eq!(
        duplicate_version_package_families(&details),
        vec!["hashbrown".to_owned()]
    );
}

#[test]
fn duplicate_budget_parser_returns_root_versions_and_direct_dependents() {
    let tree = "\
hashbrown v0.15.5
└── gpu-descriptor v0.3.2
    └── wgpu-hal v29.0.3

hashbrown v0.16.1
├── gpu-allocator v0.28.0
│   └── wgpu-hal v29.0.3
├── lru v0.16.4
└── naga v29.0.3

serde v1.0.228
└── bincode v1.3.3

serde v1.0.228
├── zbus_names v4.3.2
└── zvariant v5.11.0

tiny-skia v0.12.0 (*)
";

    let details = duplicate_package_details(tree);

    assert_eq!(
        details,
        vec![
            DuplicatePackageFamily {
                name: "hashbrown".to_owned(),
                roots: vec![
                    DuplicatePackageRoot {
                        name: "hashbrown".to_owned(),
                        version: "0.15.5".to_owned(),
                        root: "hashbrown v0.15.5".to_owned(),
                        direct_dependents: vec!["gpu-descriptor v0.3.2".to_owned()],
                    },
                    DuplicatePackageRoot {
                        name: "hashbrown".to_owned(),
                        version: "0.16.1".to_owned(),
                        root: "hashbrown v0.16.1".to_owned(),
                        direct_dependents: vec![
                            "gpu-allocator v0.28.0".to_owned(),
                            "lru v0.16.4".to_owned(),
                            "naga v29.0.3".to_owned(),
                        ],
                    },
                ],
            },
            DuplicatePackageFamily {
                name: "serde".to_owned(),
                roots: vec![
                    DuplicatePackageRoot {
                        name: "serde".to_owned(),
                        version: "1.0.228".to_owned(),
                        root: "serde v1.0.228".to_owned(),
                        direct_dependents: vec!["bincode v1.3.3".to_owned()],
                    },
                    DuplicatePackageRoot {
                        name: "serde".to_owned(),
                        version: "1.0.228".to_owned(),
                        root: "serde v1.0.228".to_owned(),
                        direct_dependents: vec![
                            "zbus_names v4.3.2".to_owned(),
                            "zvariant v5.11.0".to_owned(),
                        ],
                    },
                ],
            },
        ]
    );
}

#[test]
fn cargo_tree_package_parser_distinguishes_pixels_facade_from_external_pixels() {
    let tree = "\
cranpose v0.1.0
└── cranpose-render-pixels v0.1.0
    ├── cranpose-render-common v0.1.0
    └── ab_glyph v0.2.32
";

    assert_eq!(
        package_names_in_cargo_tree(tree),
        vec![
            "ab_glyph".to_owned(),
            "cranpose".to_owned(),
            "cranpose-render-common".to_owned(),
            "cranpose-render-pixels".to_owned()
        ]
    );
}

#[test]
fn renderer_pixels_feature_boundary_accepts_in_tree_renderer() {
    let tree = "\
cranpose v0.1.0
└── cranpose-render-pixels v0.1.0
    └── cranpose-render-common v0.1.0
";

    assert_eq!(renderer_pixels_feature_boundary_violation(tree), Ok(()));
}

#[test]
fn renderer_pixels_feature_boundary_rejects_external_pixels_stack() {
    let tree = "\
cranpose v0.1.0
├── cranpose-render-pixels v0.1.0
└── pixels v0.17.0
    └── wgpu v29.0.3
        ├── naga v29.0.3
        └── wgpu-hal v29.0.3
";

    let error = renderer_pixels_feature_boundary_violation(tree)
        .expect_err("external pixels stack should be rejected");

    assert!(error.contains("pixels"));
    assert!(error.contains("wgpu"));
    assert!(error.contains("wgpu-hal"));
    assert!(!error.contains("cranpose-render-pixels"));
}

#[test]
fn create_bundle_writes_expected_layout() {
    let workspace = unique_temp_dir();
    let binary = workspace.join("target/release/desktop-app");
    fs::create_dir_all(binary.parent().expect("binary parent")).expect("create binary parent");
    fs::write(&binary, b"demo").expect("write test binary");
    make_executable(&binary).expect("make binary executable");

    let resources = workspace.join("resources");
    fs::create_dir_all(resources.join("nested")).expect("create resources");
    fs::write(resources.join("nested/data.txt"), b"resource").expect("write resource");

    let options = BundleMacosOptions {
        package: "desktop-app".to_owned(),
        bin: "desktop-app".to_owned(),
        profile: "release".to_owned(),
        app_name: "Cranpose Demo".to_owned(),
        bundle_id: "io.cranpose.demo".to_owned(),
        out_dir: PathBuf::from("bundles"),
        resources: Some(resources),
        target: None,
        build: false,
        sign_identity: None,
    };

    let bundle = create_bundle(&workspace, &options, &binary).expect("create bundle");

    assert!(bundle.join("Contents/Info.plist").exists());
    assert!(bundle.join("Contents/MacOS/Cranpose-Demo").exists());
    assert!(bundle.join("Contents/Resources/nested/data.txt").exists());
}

#[test]
fn bundle_defaults_to_adhoc_signing() {
    assert_eq!(bundle_sign_identity(None), "-");
}

#[test]
fn bundle_uses_supplied_signing_identity() {
    assert_eq!(
        bundle_sign_identity(Some("Developer ID Application: Example")),
        "Developer ID Application: Example"
    );
}

#[test]
fn patched_isolated_build_never_touches_the_tracked_package() {
    let workspace = unique_temp_dir();
    let package_dir = workspace.join("apps/isolated-demo");
    fs::create_dir_all(package_dir.join("src")).expect("create package");
    fs::write(package_dir.join("Cargo.toml"), MINIMAL_MANIFEST).expect("write manifest");
    fs::write(package_dir.join("Cargo.lock"), PUBLISHED_LOCKFILE).expect("write lockfile");
    fs::write(package_dir.join("src/main.rs"), b"fn main() {}").expect("write source");
    fs::create_dir_all(package_dir.join("android/app/build")).expect("create gradle output");
    fs::write(package_dir.join("android/app/build/huge.apk"), b"output").expect("write output");

    let options = CargoBinaryOptions {
        package: "isolated-demo".to_owned(),
        bin: "isolated-demo".to_owned(),
        profile: "release-small".to_owned(),
        target: None,
        manifest_path: Some(PathBuf::from("apps/isolated-demo/Cargo.toml")),
        patch_workspace_cranpose: true,
    };

    let staged = stage_patched_package(&workspace, &options).expect("stage patched package");

    let staged_dir = workspace.join(PATCHED_PACKAGE_STAGE).join("isolated-demo");
    assert_eq!(
        staged.manifest_path,
        Some(staged_dir.join("Cargo.toml")),
        "a patched build must not compile the tracked manifest"
    );
    assert_eq!(
        fs::read_to_string(package_dir.join("Cargo.lock")).expect("read tracked lockfile"),
        PUBLISHED_LOCKFILE,
        "the tracked lockfile must survive staging byte for byte"
    );
    assert_eq!(
        fs::read_to_string(staged_dir.join("Cargo.lock")).expect("read staged lockfile"),
        PUBLISHED_LOCKFILE
    );
    assert!(staged_dir.join("src/main.rs").exists());
    assert!(
        !staged_dir.join("android").exists(),
        "staging must copy cargo targets, not another build system's output"
    );
}

#[test]
fn staging_is_incremental_and_prunes_removed_sources() {
    let package_dir = unique_temp_dir().join("package");
    let source = package_dir.join("src");
    fs::create_dir_all(source.join("nested")).expect("create sources");
    fs::write(package_dir.join("Cargo.toml"), b"[package]").expect("write manifest");
    fs::write(source.join("main.rs"), b"fn main() {}").expect("write source");
    fs::write(source.join("nested/gone.rs"), b"mod gone;").expect("write source");

    let staged = package_dir.join("staged");
    let roots = [source.clone()];
    stage_package(&package_dir, &staged, &roots).expect("stage package");
    let first = fs::metadata(staged.join("src/main.rs"))
        .and_then(|metadata| metadata.modified())
        .expect("staged mtime");

    stage_package(&package_dir, &staged, &roots).expect("restage package");
    let second = fs::metadata(staged.join("src/main.rs"))
        .and_then(|metadata| metadata.modified())
        .expect("restaged mtime");
    assert_eq!(first, second, "an unchanged source must not be recopied");

    fs::remove_dir_all(source.join("nested")).expect("remove sources");
    fs::remove_file(package_dir.join("Cargo.toml")).expect("remove manifest");
    stage_package(&package_dir, &staged, &roots).expect("restage pruned package");
    assert!(
        !staged.join("src/nested").exists(),
        "staging must drop sources the package no longer has"
    );
    assert!(!staged.join("Cargo.toml").exists());
}

#[test]
fn unpatched_builds_compile_the_manifest_they_were_given() {
    let workspace = unique_temp_dir();
    let options = CargoBinaryOptions {
        manifest_path: Some(PathBuf::from("apps/isolated-demo/Cargo.toml")),
        ..CargoBinaryOptions::default()
    };

    let staged = stage_patched_package(&workspace, &options).expect("stage unpatched package");

    assert_eq!(staged, options);
}

#[test]
fn workspace_builds_need_no_staging() {
    let workspace = unique_temp_dir();
    let options = CargoBinaryOptions {
        patch_workspace_cranpose: true,
        ..CargoBinaryOptions::default()
    };

    let staged = stage_patched_package(&workspace, &options).expect("stage workspace build");

    assert_eq!(staged, options);
}

#[test]
fn target_source_roots_dedupe_the_directories_cargo_reports() {
    let metadata = r#"{"packages":[{"targets":[
        {"name":"isolated_demo","src_path":"/w/apps/isolated-demo/src/lib.rs"},
        {"name":"isolated-demo","src_path":"/w/apps/isolated-demo/src/main.rs"},
        {"name":"build-script-build","src_path":"/w/apps/isolated-demo/build.rs"}
    ]}]}"#;

    assert_eq!(
        parse_target_source_roots(metadata),
        vec![
            PathBuf::from("/w/apps/isolated-demo/src"),
            PathBuf::from("/w/apps/isolated-demo"),
        ]
    );
}

#[test]
fn staging_rejects_targets_outside_the_package() {
    let root = unique_temp_dir();
    let package_dir = root.join("package");
    fs::create_dir_all(&package_dir).expect("create package");

    let error = stage_package(
        &package_dir,
        &root.join("staged"),
        &[root.join("elsewhere")],
    )
    .expect_err("a target outside the package must not be staged silently");

    assert!(error.contains("lies outside package"), "{error}");
}

const FIXTURE_VERSION: &str = "0.1.105";

fn write_root_manifest(root: &Path, workspace_version: &str, dependency_version: &str) {
    let manifest = format!(
        "[workspace]\n\
         members = [\"crates/cranpose\"]\n\
         \n\
         [workspace.package]\n\
         version = \"{workspace_version}\"\n\
         \n\
         [workspace.dependencies]\n\
         cranpose = {{ path = \"crates/cranpose\", version = \"{dependency_version}\" }}\n"
    );
    fs::write(root.join("Cargo.toml"), manifest).expect("write root manifest");
}

/// A root `Cargo.lock` entry for a workspace member: no `source` or
/// `checksum`, exactly like a path dependency the workspace resolves
/// itself (see the real `Cargo.lock`, which carries neither for its own
/// `cranpose` packages).
fn write_root_lock(root: &Path, version: &str) {
    let lock =
        format!("version = 4\n\n[[package]]\nname = \"cranpose\"\nversion = \"{version}\"\n");
    fs::write(root.join("Cargo.lock"), lock).expect("write root lock");
}

fn write_root_lock_without_cranpose(root: &Path) {
    let lock = "version = 4\n\n[[package]]\nname = \"log\"\nversion = \"0.4.0\"\n";
    fs::write(root.join("Cargo.lock"), lock).expect("write root lock");
}

fn write_isolated_manifest(root: &Path, dependency_version: &str) {
    let dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&dir).expect("create apps/isolated-demo");
    let manifest = format!(
        "[package]\n\
         name = \"isolated-demo\"\n\
         version = \"0.1.0\"\n\
         \n\
         [dependencies]\n\
         cranpose = {{ version = \"{dependency_version}\" }}\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).expect("write isolated-demo manifest");
}

fn write_isolated_lock(root: &Path, version: &str, source: Option<&str>, checksum: Option<&str>) {
    let dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&dir).expect("create apps/isolated-demo");
    let mut lock =
        format!("version = 4\n\n[[package]]\nname = \"cranpose\"\nversion = \"{version}\"\n");
    if let Some(source) = source {
        lock.push_str(&format!("source = \"{source}\"\n"));
    }
    if let Some(checksum) = checksum {
        lock.push_str(&format!("checksum = \"{checksum}\"\n"));
    }
    fs::write(dir.join("Cargo.lock"), lock).expect("write isolated-demo lock");
}

fn write_isolated_lock_without_cranpose(root: &Path) {
    let dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&dir).expect("create apps/isolated-demo");
    let lock = "version = 4\n\n[[package]]\nname = \"log\"\nversion = \"0.4.0\"\n";
    fs::write(dir.join("Cargo.lock"), lock).expect("write isolated-demo lock");
}

/// Every file `check_versions_at` reads, all agreeing at `FIXTURE_VERSION`.
fn write_aligned_versions_fixture(root: &Path) {
    write_root_manifest(root, FIXTURE_VERSION, FIXTURE_VERSION);
    write_root_lock(root, FIXTURE_VERSION);
    write_isolated_manifest(root, FIXTURE_VERSION);
    write_isolated_lock(
        root,
        FIXTURE_VERSION,
        Some(CRATES_IO_SOURCE),
        Some("deadbeef"),
    );
}

#[test]
fn versions_pass_when_everything_aligned() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);

    check_versions_at(&root).expect("aligned versions must pass");
}

/// Fixtures prove the logic; they cannot prove the *real* Cargo.toml
/// still parses the way the fixtures assume it does. A fixture-only
/// suite can stay green while the actual manifest drifts into a shape
/// this parser does not handle (a renamed section, a moved key) and the
/// gate would silently stop meaning anything on the one file it exists
/// to check.
fn real_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("resolve the real workspace root from xtask's own manifest dir")
}

#[test]
fn check_versions_passes_against_the_real_workspace() {
    check_versions_at(&real_workspace_root())
        .expect("the real workspace's own versions must already be aligned");
}

/// The root manifest's `[workspace]` table.
fn workspace_table(root: &Path) -> toml::value::Table {
    load_toml(&root.join("Cargo.toml"))
        .expect("parse the workspace manifest")
        .get("workspace")
        .and_then(toml::Value::as_table)
        .cloned()
        .expect("the root manifest has a [workspace] table")
}

/// Every path listed in `workspace.members`.
fn workspace_members(workspace: &toml::value::Table) -> Vec<String> {
    workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .expect("the workspace manifest lists its members")
        .iter()
        .map(|member| {
            member
                .as_str()
                .expect("every workspace member is a path string")
                .to_owned()
        })
        .collect()
}

/// The level `[workspace.lints.rust]` sets for `name`, if any.
fn workspace_rust_lint_level(workspace: &toml::value::Table, name: &str) -> Option<String> {
    workspace
        .get("lints")
        .and_then(toml::Value::as_table)
        .and_then(|lints| lints.get("rust"))
        .and_then(toml::Value::as_table)
        .and_then(|rust| rust.get(name))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
}

/// The rules AGENTS.md hands to the compiler live in the root manifest's
/// `[workspace.lints]` tables, and cargo applies them to a member only
/// when that member's own manifest says `[lints] workspace = true`.
/// Nothing requires that opt-in, so a crate added without it builds with
/// `unsafe_code`, `todo`, `dbg_macro` and `linker_messages` all back at
/// their default levels while every gate stays green.
#[test]
fn every_workspace_member_inherits_the_workspace_lints() {
    let root = real_workspace_root();
    let missing: Vec<String> = workspace_members(&workspace_table(&root))
        .into_iter()
        .filter(|member| {
            let manifest = load_toml(&root.join(member).join("Cargo.toml"))
                .expect("parse a workspace member manifest");
            manifest
                .get("lints")
                .and_then(toml::Value::as_table)
                .and_then(|lints| lints.get("workspace"))
                .and_then(toml::Value::as_bool)
                != Some(true)
        })
        .collect();

    assert!(
        missing.is_empty(),
        "these workspace members do not carry `workspace = true` under `[lints]`, so the \
         workspace lint table does not reach them: {missing:?}"
    );
}

/// `linker_messages` is the one warning class a `-D warnings` run cannot
/// reach, because clippy stops at metadata and never links. Denying it
/// in the workspace lint table is what fails a build on a linker
/// warning; at its default level a message such as macOS ld's
/// `__eh_frame section too large` prints under `cargo build`/`cargo
/// test` and turns nothing red.
#[test]
fn the_workspace_lints_deny_linker_messages() {
    assert_eq!(
        workspace_rust_lint_level(&workspace_table(&real_workspace_root()), "linker_messages")
            .as_deref(),
        Some("deny"),
        "a linker warning must fail the build; shrink what is being linked instead of \
         relaxing this level -- TIME_WASTERS.md records how to attribute an oversized \
         `__eh_frame` back to the objects that filled it"
    );
}

#[test]
fn verify_tag_passes_against_the_real_workspace_version() {
    let root = real_workspace_root();
    let workspace_version =
        workspace_package_version(&root).expect("read the real workspace version");

    verify_tag_at(&root, &format!("v{workspace_version}"))
        .expect("verify-tag must accept the real workspace's own version as a matching tag");
}

#[test]
fn versions_fail_when_workspace_dependency_diverges() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_root_manifest(&root, FIXTURE_VERSION, "0.1.104");

    let error = check_versions_at(&root).expect_err("a stale workspace dependency must fail");

    assert!(
        error.contains("workspace dependency cranpose is 0.1.104, expected 0.1.105"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_lockfile_version_diverges() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_root_lock(&root, "0.1.104");

    let error = check_versions_at(&root).expect_err("a stale Cargo.lock entry must fail");

    assert!(
        error.contains("Cargo.lock package cranpose has 0.1.104, expected 0.1.105"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_lockfile_is_missing_a_workspace_package() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_root_lock_without_cranpose(&root);

    let error = check_versions_at(&root).expect_err("a missing lock entry must fail");

    assert!(
        error.contains("Cargo.lock is missing workspace package cranpose"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_isolated_demo_manifest_diverges() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_isolated_manifest(&root, "0.1.104");

    let error = check_versions_at(&root).expect_err("a stale isolated-demo manifest must fail");

    assert!(
        error.contains("apps/isolated-demo dependency cranpose is 0.1.104, expected 0.1.105"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_isolated_lock_has_no_cranpose_packages() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_isolated_lock_without_cranpose(&root);

    let error =
        check_versions_at(&root).expect_err("a canary lockfile with no cranpose crates must fail");

    assert!(
        error.contains("apps/isolated-demo/Cargo.lock locks no cranpose packages"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_isolated_lock_resolves_from_a_local_path() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_isolated_lock(&root, FIXTURE_VERSION, None, None);

    let error =
        check_versions_at(&root).expect_err("a patched/path-resolved canary lockfile must fail");

    assert!(
        error.contains(
            "apps/isolated-demo/Cargo.lock resolves cranpose from a local path, expected \
             the published crate at registry+https://github.com/rust-lang/crates.io-index"
        ),
        "{error}"
    );
}

#[test]
fn versions_fail_when_isolated_lock_resolves_from_a_patch() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_isolated_lock(
        &root,
        FIXTURE_VERSION,
        Some("git+https://example.com/cranpose"),
        None,
    );

    let error = check_versions_at(&root).expect_err("a git-resolved canary lockfile must fail");

    assert!(
        error.contains("resolves cranpose from git+https://example.com/cranpose, expected"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_isolated_lock_has_no_checksum() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_isolated_lock(&root, FIXTURE_VERSION, Some(CRATES_IO_SOURCE), None);

    let error =
        check_versions_at(&root).expect_err("a canary lockfile missing a checksum must fail");

    assert!(
        error.contains("apps/isolated-demo/Cargo.lock package cranpose has no checksum"),
        "{error}"
    );
}

#[test]
fn versions_fail_when_isolated_lock_version_diverges() {
    let root = unique_temp_dir();
    write_aligned_versions_fixture(&root);
    write_isolated_lock(&root, "0.1.104", Some(CRATES_IO_SOURCE), Some("deadbeef"));

    let error = check_versions_at(&root).expect_err("a stale canary lockfile version must fail");

    assert!(
        error.contains(
            "apps/isolated-demo/Cargo.lock package cranpose is 0.1.104, expected 0.1.105"
        ),
        "{error}"
    );
}

#[test]
fn dependency_version_reads_bare_strings_and_inline_tables() {
    let bare: toml::Value = toml::from_str("v = \"1.2.3\"").expect("parse bare string");
    let table: toml::Value =
        toml::from_str("v = { path = \"crates/cranpose\", version = \"1.2.3\" }")
            .expect("parse inline table");
    let no_version: toml::Value =
        toml::from_str("v = { path = \"crates/cranpose\" }").expect("parse table without version");

    assert_eq!(
        dependency_version(bare.get("v").expect("v present")),
        Some("1.2.3".to_owned())
    );
    assert_eq!(
        dependency_version(table.get("v").expect("v present")),
        Some("1.2.3".to_owned())
    );
    assert_eq!(
        dependency_version(no_version.get("v").expect("v present")),
        None
    );
}

#[test]
fn lock_versions_groups_by_package_name() {
    let lock: toml::Value = toml::from_str(
        "[[package]]\n\
         name = \"cranpose\"\n\
         version = \"0.1.0\"\n\
         \n\
         [[package]]\n\
         name = \"cranpose\"\n\
         version = \"0.1.1\"\n\
         \n\
         [[package]]\n\
         name = \"log\"\n\
         version = \"0.4.0\"\n",
    )
    .expect("parse lockfile");

    let versions = lock_versions(&lock_packages(&lock, |name| name.starts_with("cranpose")));

    assert_eq!(versions.len(), 1, "only cranpose-prefixed packages count");
    assert_eq!(
        versions.get("cranpose"),
        Some(&BTreeSet::from(["0.1.0".to_owned(), "0.1.1".to_owned()]))
    );
}

fn sync_isolated_demo_to(root: &Path, version: &str) {
    sync_isolated_demo_at(
        root,
        SyncIsolatedDemoOptions {
            version: Some(version.to_owned()),
        },
    )
    .expect("sync must succeed");
}

#[test]
fn sync_isolated_demo_rewrites_inline_and_bare_dependencies() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    let manifest_dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[dependencies]\n\
         cranpose = { version = \"0.1.104\" }\n\
         cranpose-core = \"0.1.104\"\n\
         coroflow = \"0.1.104\"\n\
         log = \"0.4\"\n\
         \n\
         [target.'cfg(target_arch = \"wasm32\")'.dependencies]\n\
         cranpose-platform-web = \"0.1.104\"\n",
    )
    .expect("write isolated-demo manifest");

    sync_isolated_demo_to(&root, "0.1.105");

    let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
    assert!(
        updated.contains("cranpose = { version = \"0.1.105\" }"),
        "{updated}"
    );
    assert!(updated.contains("cranpose-core = \"0.1.105\""), "{updated}");
    assert!(
        updated.contains("coroflow = \"0.1.105\""),
        "coroflow is released with Cranpose: {updated}"
    );
    assert!(
        updated.contains("cranpose-platform-web = \"0.1.105\""),
        "target-cfg dependencies must be rewritten too: {updated}"
    );
    assert!(
        updated.contains("log = \"0.4\""),
        "non-cranpose dependencies must be left alone: {updated}"
    );
}

#[test]
fn sync_isolated_demo_is_a_noop_when_already_synced() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    let manifest_dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
    let manifest_text = "[dependencies]\ncranpose = { version = \"0.1.105\" }\n";
    fs::write(manifest_dir.join("Cargo.toml"), manifest_text).expect("write manifest");

    sync_isolated_demo_to(&root, "0.1.105");

    let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
    assert_eq!(
        updated, manifest_text,
        "an already-synced manifest must be left byte-for-byte alone"
    );
}

#[test]
fn sync_isolated_demo_defaults_to_the_workspace_version() {
    let root = unique_temp_dir();
    write_root_manifest(&root, "0.1.107", "0.1.107");
    let manifest_dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[dependencies]\ncranpose = { version = \"0.1.104\" }\n",
    )
    .expect("write manifest");

    sync_isolated_demo_at(&root, SyncIsolatedDemoOptions { version: None })
        .expect("sync must succeed");

    let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
    assert!(updated.contains("0.1.107"), "{updated}");
}

#[test]
fn sync_isolated_demo_strips_a_leading_v() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    let manifest_dir = root.join("apps/isolated-demo");
    fs::create_dir_all(&manifest_dir).expect("create apps/isolated-demo");
    fs::write(
        manifest_dir.join("Cargo.toml"),
        "[dependencies]\ncranpose = { version = \"0.1.104\" }\n",
    )
    .expect("write manifest");

    sync_isolated_demo_at(
        &root,
        SyncIsolatedDemoOptions {
            version: Some("v0.2.0".to_owned()),
        },
    )
    .expect("sync must succeed");

    let updated = fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("read manifest");
    assert!(updated.contains("0.2.0"), "{updated}");
    assert!(
        !updated.contains("v0.2.0"),
        "the leading v must be stripped: {updated}"
    );
}

#[test]
fn sync_isolated_demo_rejects_a_non_semver_version() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    write_isolated_manifest(&root, FIXTURE_VERSION);

    let error = sync_isolated_demo_at(
        &root,
        SyncIsolatedDemoOptions {
            version: Some("not-a-version".to_owned()),
        },
    )
    .expect_err("a non-semver version must be rejected");

    assert!(error.contains("Not a semver version"), "{error}");
}

#[test]
fn semver_regex_accepts_prerelease_and_build_metadata() {
    assert!(SEMVER_RE.is_match("1.2.3"));
    assert!(SEMVER_RE.is_match("1.2.3-alpha.1"));
    assert!(SEMVER_RE.is_match("1.2.3+build.5"));
    assert!(!SEMVER_RE.is_match("1.2"));
    assert!(!SEMVER_RE.is_match("v1.2.3"));
    assert!(!SEMVER_RE.is_match("not-a-version"));
}

#[test]
fn parse_sync_isolated_demo_options() {
    let none = SyncIsolatedDemoOptions::parse(&[]).expect("no args parse");
    assert_eq!(none.version, None);

    let with_version =
        SyncIsolatedDemoOptions::parse(&["0.1.73".to_owned()]).expect("one arg parses");
    assert_eq!(with_version.version.as_deref(), Some("0.1.73"));

    SyncIsolatedDemoOptions::parse(&["0.1.73".to_owned(), "0.1.74".to_owned()])
        .expect_err("two positional arguments must be rejected");
    SyncIsolatedDemoOptions::parse(&["--bogus".to_owned()])
        .expect_err("unknown flags must be rejected");
}

fn write_root_lock_with_multiple_packages(root: &Path, cranpose_version: &str) {
    let lock = format!(
        "version = 4\n\
         \n\
         [[package]]\n\
         name = \"cranpose\"\n\
         version = \"{cranpose_version}\"\n\
         dependencies = [\n\
         \x20\"log\",\n\
         ]\n\
         \n\
         [[package]]\n\
         name = \"cranpose-core\"\n\
         version = \"{cranpose_version}\"\n\
         \n\
         [[package]]\n\
         name = \"log\"\n\
         version = \"0.4.0\"\n"
    );
    fs::write(root.join("Cargo.lock"), lock).expect("write root lock");
}

#[test]
fn bump_release_version_rewrites_toml_and_lock() {
    let root = unique_temp_dir();
    write_root_manifest(&root, "0.1.104", "0.1.104");
    write_root_lock_with_multiple_packages(&root, "0.1.104");

    bump_release_version_at(&root, "v0.1.105").expect("bump must succeed");

    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    assert!(
        manifest.contains("version = \"0.1.105\""),
        "workspace.package.version must be bumped: {manifest}"
    );
    assert!(
        manifest.contains("cranpose = { path = \"crates/cranpose\", version = \"0.1.105\" }"),
        "workspace dependency must be bumped: {manifest}"
    );

    let lock = fs::read_to_string(root.join("Cargo.lock")).expect("read lock");
    assert!(
        lock.contains("name = \"cranpose\"\nversion = \"0.1.105\""),
        "{lock}"
    );
    assert!(
        lock.contains("name = \"cranpose-core\"\nversion = \"0.1.105\""),
        "{lock}"
    );
    assert!(
        lock.contains("name = \"log\"\nversion = \"0.4.0\""),
        "a non-cranpose package must be left alone: {lock}"
    );
}

#[test]
fn bump_release_version_rejects_a_tag_without_a_leading_v() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    write_root_lock(&root, FIXTURE_VERSION);

    let error = bump_release_version_at(&root, "0.1.105")
        .expect_err("a tag without a leading v must be rejected");

    assert_eq!(
        error,
        format!("Malformed release tag '0.1.105': expected the shape {RELEASE_TAG_SHAPE}")
    );
}

#[test]
fn release_version_from_tag_takes_one_v_and_three_numbers() {
    assert_eq!(release_version_from_tag("v0.1.132"), Ok("0.1.132"));
    assert_eq!(release_version_from_tag("v1.10.0"), Ok("1.10.0"));

    for tag in [
        "vv0.1.132",
        "0.1.132",
        "0.1",
        "v0.1",
        "v0.1.132-rc1",
        "v0.1.x",
        "v",
        "v0.1.2.3",
    ] {
        let error = release_version_from_tag(tag).expect_err("only v and three numbers may pass");
        assert_eq!(
            error,
            format!("Malformed release tag '{tag}': expected the shape {RELEASE_TAG_SHAPE}")
        );
    }
}

#[test]
fn bump_release_version_rejects_a_doubled_v_and_writes_nothing() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    write_root_lock(&root, FIXTURE_VERSION);
    let manifest_before = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    let lock_before = fs::read_to_string(root.join("Cargo.lock")).expect("read lock");

    let error =
        bump_release_version_at(&root, "vv0.1.132").expect_err("a doubled v must be rejected");

    assert_eq!(
        error,
        "Malformed release tag 'vv0.1.132': expected the shape v<major>.<minor>.<patch>, as in v0.1.132"
    );
    assert_eq!(
        fs::read_to_string(root.join("Cargo.toml")).expect("read manifest"),
        manifest_before
    );
    assert_eq!(
        fs::read_to_string(root.join("Cargo.lock")).expect("read lock"),
        lock_before
    );
}

#[test]
fn bump_release_version_leaves_cargo_toml_untouched_on_a_dependency_mismatch() {
    let root = unique_temp_dir();
    let manifest = "[workspace]\n\
         members = [\"crates/cranpose\"]\n\
         \n\
         [workspace.package]\n\
         version = \"0.1.104\"\n\
         \n\
         [workspace.dependencies]\n\
         cranpose = { path = \"crates/cranpose\" }\n";
    fs::write(root.join("Cargo.toml"), manifest).expect("write manifest");
    write_root_lock(&root, "0.1.104");

    let error = bump_release_version_at(&root, "v0.1.105")
        .expect_err("a dependency with no version key must be reported, not silently kept");

    assert!(
        error.contains("Some release workspace dependencies were not updated"),
        "{error}"
    );
    let unchanged = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    assert_eq!(
        unchanged, manifest,
        "Cargo.toml must not be written when the dependency pass fails"
    );
}

#[test]
fn bump_release_version_creates_workspace_package_when_missing() {
    let root = unique_temp_dir();
    let manifest = "[workspace]\n\
         members = [\"crates/cranpose\"]\n\
         \n\
         [workspace.dependencies]\n\
         cranpose = { path = \"crates/cranpose\", version = \"0.1.104\" }\n";
    fs::write(root.join("Cargo.toml"), manifest).expect("write manifest");
    write_root_lock(&root, "0.1.104");

    bump_release_version_at(&root, "v0.1.105").expect("bump must succeed");

    let updated = fs::read_to_string(root.join("Cargo.toml")).expect("read manifest");
    assert!(updated.contains("[workspace.package]"), "{updated}");
    assert!(updated.contains("version = \"0.1.105\""), "{updated}");
    assert!(
        updated.contains("cranpose = { path = \"crates/cranpose\", version = \"0.1.105\" }"),
        "{updated}"
    );
}

#[test]
fn verify_tag_passes_without_touching_the_lockfile_or_isolated_demo() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);
    // Deliberately no Cargo.lock and no apps/isolated-demo: this runs
    // between `sync_versions` and `bump_isolated_demo`, where the demo
    // still points at the previous release on purpose. If verify-tag
    // read either, this test would fail with a missing-file error.

    verify_tag_at(&root, "v0.1.105").expect("verify-tag must not need Cargo.lock or the demo");
}

#[test]
fn verify_tag_rejects_a_tag_without_a_leading_v() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);

    let error = verify_tag_at(&root, "0.1.105").expect_err("a tag without a leading v must fail");

    assert_eq!(
        error,
        format!("Malformed release tag '0.1.105': expected the shape {RELEASE_TAG_SHAPE}")
    );
}

#[test]
fn verify_tag_rejects_a_doubled_v() {
    let root = unique_temp_dir();
    write_root_manifest(&root, FIXTURE_VERSION, FIXTURE_VERSION);

    let error = verify_tag_at(&root, "vv0.1.105").expect_err("a doubled v must fail");

    assert_eq!(
        error,
        format!("Malformed release tag 'vv0.1.105': expected the shape {RELEASE_TAG_SHAPE}")
    );
}

#[test]
fn verify_tag_fails_when_tag_does_not_match_workspace_version() {
    let root = unique_temp_dir();
    write_root_manifest(&root, "0.1.105", "0.1.105");

    let error = verify_tag_at(&root, "v0.1.106")
        .expect_err("a tag ahead of the workspace version must fail");

    assert_eq!(
        error,
        "Tag version v0.1.106 does not match workspace version 0.1.105"
    );
}

#[test]
fn verify_tag_fails_when_a_workspace_dependency_diverges() {
    let root = unique_temp_dir();
    write_root_manifest(&root, "0.1.105", "0.1.104");

    let error =
        verify_tag_at(&root, "v0.1.105").expect_err("a stale workspace dependency must fail");

    assert_eq!(
        error,
        "Workspace dependency versions must match workspace version:\ncranpose => 0.1.104"
    );
}

const PUBLISH_ORDER_METADATA_TEMPLATE: &str = r#"{
    "workspace_members": ["cranpose-core 0.1.0", "cranpose 0.1.0", "cranpose-ui 0.1.0"],
    "packages": [
        {
            "id": "cranpose-core 0.1.0",
            "name": "cranpose-core",
            "dependencies": []
        },
        {
            "id": "cranpose 0.1.0",
            "name": "cranpose",
            "dependencies": [
                {"name": "cranpose-core", "kind": null},
                {"name": "cranpose-ui", "kind": null}
            ]
        },
        {
            "id": "cranpose-ui 0.1.0",
            "name": "cranpose-ui",
            "dependencies": [
                {"name": "cranpose-core", "kind": null}
            ]
        },
        {
            "id": "log 0.4.0",
            "name": "log",
            "dependencies": []
        }
    ]
}"#;

#[test]
fn resolve_publish_order_orders_dependencies_before_dependents() {
    let order = resolve_publish_order(PUBLISH_ORDER_METADATA_TEMPLATE)
        .expect("a valid dependency graph must resolve");

    assert_eq!(order, vec!["cranpose-core", "cranpose-ui", "cranpose"]);
}

#[test]
fn resolve_publish_order_ignores_dev_dependencies() {
    // liquid -> testing (dev) -> cranpose -> liquid would be a cycle if
    // dev-deps gated the order; they must not.
    let metadata = r#"{
        "workspace_members": ["cranpose-liquid 0.1.0", "cranpose-testing 0.1.0", "cranpose 0.1.0"],
        "packages": [
            {
                "id": "cranpose-liquid 0.1.0",
                "name": "cranpose-liquid",
                "dependencies": [
                    {"name": "cranpose-testing", "kind": "dev"}
                ]
            },
            {
                "id": "cranpose-testing 0.1.0",
                "name": "cranpose-testing",
                "dependencies": [
                    {"name": "cranpose", "kind": null}
                ]
            },
            {
                "id": "cranpose 0.1.0",
                "name": "cranpose",
                "dependencies": [
                    {"name": "cranpose-liquid", "kind": "dev"}
                ]
            }
        ]
    }"#;

    let order = resolve_publish_order(metadata).expect("dev-only cycles must not block");

    assert_eq!(order.len(), 3);
    assert!(
        order.iter().position(|n| n == "cranpose").unwrap()
            < order.iter().position(|n| n == "cranpose-testing").unwrap()
    );
}

#[test]
fn resolve_publish_order_rejects_a_real_cycle() {
    let metadata = r#"{
        "workspace_members": ["cranpose-a 0.1.0", "cranpose-b 0.1.0"],
        "packages": [
            {
                "id": "cranpose-a 0.1.0",
                "name": "cranpose-a",
                "dependencies": [{"name": "cranpose-b", "kind": null}]
            },
            {
                "id": "cranpose-b 0.1.0",
                "name": "cranpose-b",
                "dependencies": [{"name": "cranpose-a", "kind": null}]
            }
        ]
    }"#;

    let error = resolve_publish_order(metadata).expect_err("a real cycle must be rejected");

    assert_eq!(
        error,
        "Cyclic cranpose publish dependencies: cranpose-a, cranpose-b"
    );
}

#[test]
fn resolve_publish_order_ignores_non_workspace_and_non_cranpose_packages() {
    let metadata = r#"{
        "workspace_members": ["cranpose 0.1.0"],
        "packages": [
            {"id": "cranpose 0.1.0", "name": "cranpose", "dependencies": [
                {"name": "log", "kind": null}
            ]},
            {"id": "log 0.4.0 (registry+https://x)", "name": "log", "dependencies": []}
        ]
    }"#;

    let order = resolve_publish_order(metadata).expect("must resolve");

    assert_eq!(order, vec!["cranpose"]);
}

const MINIMAL_MANIFEST: &str = "\
[package]
name = \"isolated-demo\"
version = \"0.1.0\"
edition = \"2021\"

[[bin]]
name = \"isolated-demo\"
path = \"src/main.rs\"

[workspace]
";

const PUBLISHED_LOCKFILE: &str = "\
version = 4

[[package]]
name = \"isolated-demo\"
version = \"0.1.0\"
";

fn strs(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).to_owned()).collect()
}

fn token_lines(lines: &[&[&str]]) -> Vec<Vec<String>> {
    lines.iter().map(|line| strs(line)).collect()
}

#[test]
fn parse_hunk_spans_single_hunk_modified_file() {
    let diff = [
        "diff --git a/src/lib.rs b/src/lib.rs",
        "--- src/lib.rs",
        "+++ src/lib.rs",
        "@@ -10,2 +10,3 @@",
        "+one",
        "+two",
        "+three",
    ]
    .join("\n");
    assert_eq!(
        gate_diff::parse_hunk_spans(&diff),
        BTreeMap::from([(
            "src/lib.rs".to_owned(),
            vec![(Some((10, 11)), Some((10, 12)))]
        )])
    );
}

#[test]
fn parse_hunk_spans_single_line_hunk_omits_count() {
    let diff = [
        "--- src/lib.rs",
        "+++ src/lib.rs",
        "@@ -5 +5 @@",
        "-old",
        "+new",
    ]
    .join("\n");
    assert_eq!(
        gate_diff::parse_hunk_spans(&diff),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((5, 5)), Some((5, 5)))])])
    );
}

#[test]
fn parse_hunk_spans_pure_deletion_hunk_has_no_new_side_span() {
    let diff = [
        "--- src/lib.rs",
        "+++ src/lib.rs",
        "@@ -20,3 +19,0 @@",
        "-gone",
        "-gone too",
        "-and this",
    ]
    .join("\n");
    assert_eq!(
        gate_diff::parse_hunk_spans(&diff),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((20, 22)), None)])])
    );
}

#[test]
fn parse_hunk_spans_pure_addition_hunk_has_no_old_side_span() {
    let diff = [
        "--- src/lib.rs",
        "+++ src/lib.rs",
        "@@ -50,0 +52,1 @@",
        "+a3",
    ]
    .join("\n");
    assert_eq!(
        gate_diff::parse_hunk_spans(&diff),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(None, Some((52, 52)))])])
    );
}

#[test]
fn parse_hunk_spans_deleted_file_is_skipped() {
    let diff = [
        "--- src/dead.rs",
        "+++ /dev/null",
        "@@ -1,3 +0,0 @@",
        "-a",
        "-b",
        "-c",
    ]
    .join("\n");
    assert_eq!(gate_diff::parse_hunk_spans(&diff), BTreeMap::new());
}

#[test]
fn parse_hunk_spans_multiple_hunks_and_files() {
    let diff = [
        "--- src/a.rs",
        "+++ src/a.rs",
        "@@ -1,0 +1,2 @@",
        "+a1",
        "+a2",
        "@@ -50,0 +52,1 @@",
        "+a3",
        "--- src/b.rs",
        "+++ src/b.rs",
        "@@ -3,1 +3,1 @@",
        "-old",
        "+new",
    ]
    .join("\n");
    assert_eq!(
        gate_diff::parse_hunk_spans(&diff),
        BTreeMap::from([
            (
                "src/a.rs".to_owned(),
                vec![(None, Some((1, 2))), (None, Some((52, 52)))]
            ),
            ("src/b.rs".to_owned(), vec![(Some((3, 3)), Some((3, 3)))]),
        ])
    );
}

#[test]
fn code_tokens_plain_code_line() {
    assert_eq!(
        gate_diff::code_tokens_by_line("let x = 1;"),
        token_lines(&[&["let", "x", "=", "1", ";"]])
    );
}

#[test]
fn code_tokens_blank_line() {
    assert_eq!(gate_diff::code_tokens_by_line(""), token_lines(&[&[]]));
    assert_eq!(
        gate_diff::code_tokens_by_line("   \t  "),
        token_lines(&[&[]])
    );
}

#[test]
fn code_tokens_pure_line_comment() {
    assert_eq!(
        gate_diff::code_tokens_by_line("// just a note"),
        token_lines(&[&[]])
    );
}

#[test]
fn code_tokens_trailing_comment_yields_only_the_codes_tokens() {
    assert_eq!(
        gate_diff::code_tokens_by_line("let x = 1; // trailing"),
        token_lines(&[&["let", "x", "=", "1", ";"]])
    );
}

#[test]
fn code_tokens_slash_slash_inside_a_string_is_one_string_token_not_a_comment() {
    let text = "let url = \"https://example.com\";";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "url", "=", "\"https://example.com\"", ";"]])
    );
}

#[test]
fn code_tokens_string_spanning_a_naive_comment_check_does_not_start_one() {
    let text = ["let s = \"a // b\";", "// this really is a comment"].join("\n");
    assert_eq!(
        gate_diff::code_tokens_by_line(&text),
        token_lines(&[&["let", "s", "=", "\"a // b\"", ";"], &[]])
    );
}

#[test]
fn code_tokens_single_line_block_comment() {
    assert_eq!(
        gate_diff::code_tokens_by_line("/* note */"),
        token_lines(&[&[]])
    );
}

#[test]
fn code_tokens_single_line_block_comment_with_trailing_code() {
    assert_eq!(
        gate_diff::code_tokens_by_line("/* note */ let x = 1;"),
        token_lines(&[&["let", "x", "=", "1", ";"]])
    );
}

#[test]
fn code_tokens_multi_line_block_comment_is_all_blank() {
    let text = ["/* start", "middle line", "end */"].join("\n");
    assert_eq!(
        gate_diff::code_tokens_by_line(&text),
        token_lines(&[&[], &[], &[]])
    );
}

#[test]
fn code_tokens_multi_line_block_comment_with_code_before_and_after() {
    let text = ["let a = 1; /* start", "middle line", "end */ let b = 2;"].join("\n");
    assert_eq!(
        gate_diff::code_tokens_by_line(&text),
        token_lines(&[
            &["let", "a", "=", "1", ";"],
            &[],
            &["let", "b", "=", "2", ";"]
        ])
    );
}

#[test]
fn code_tokens_nested_block_comments() {
    let text = ["/* outer /* inner */ still commented", "*/ let x = 1;"].join("\n");
    assert_eq!(
        gate_diff::code_tokens_by_line(&text),
        token_lines(&[&[], &["let", "x", "=", "1", ";"]])
    );
}

#[test]
fn code_tokens_raw_string_containing_slashes_and_quotes_is_one_token() {
    let text = "let s = r#\"// not a comment, and \"quoted\" too\"#;";
    let raw_token = "r#\"// not a comment, and \"quoted\" too\"#";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "s", "=", raw_token, ";"]])
    );
}

#[test]
fn code_tokens_raw_string_spanning_lines_is_one_token_on_its_start_line() {
    let text = "let s = r\"line one\n// still string content\nline three\";";
    let raw_token = "r\"line one\n// still string content\nline three\"";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "s", "=", raw_token], &[], &[";"]])
    );
}

#[test]
fn code_tokens_raw_string_prefix_not_misdetected_mid_identifier() {
    let text = "let bar = 1; let s = \"text\";";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "bar", "=", "1", ";", "let", "s", "=", "\"text\"", ";"]])
    );
}

#[test]
fn code_tokens_char_literal_does_not_start_a_comment() {
    let text = "let c = '/';";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "c", "=", "'/'", ";"]])
    );
}

#[test]
fn code_tokens_escaped_char_literal() {
    let text = "let c = '\\n';";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "c", "=", "'\\n'", ";"]])
    );
}

#[test]
fn code_tokens_lifetime_is_not_treated_as_an_unterminated_char_literal() {
    let text = "fn f<'a>(x: &'a str) -> &'a str { x }\n// a real comment";
    let tokens = gate_diff::code_tokens_by_line(text);
    assert!(
        !tokens[0].is_empty(),
        "the code line must yield at least one token"
    );
    assert_eq!(tokens[1], Vec::<String>::new());
}

#[test]
fn code_tokens_string_containing_an_escaped_quote_is_one_token() {
    let text = "let s = \"she said \\\"hi\\\"\";";
    let string_token = "\"she said \\\"hi\\\"\"";
    assert_eq!(
        gate_diff::code_tokens_by_line(text),
        token_lines(&[&["let", "s", "=", string_token, ";"]])
    );
}

#[test]
fn drop_trailing_commas_comma_before_closing_paren_is_dropped() {
    assert_eq!(
        gate_diff::drop_trailing_commas(&strs(&["f", "(", "a", ",", ")"])),
        strs(&["f", "(", "a", ")"])
    );
}

#[test]
fn drop_trailing_commas_comma_before_closing_bracket_is_dropped() {
    assert_eq!(
        gate_diff::drop_trailing_commas(&strs(&["[", "1", ",", "]"])),
        strs(&["[", "1", "]"])
    );
}

#[test]
fn drop_trailing_commas_comma_before_closing_brace_is_dropped() {
    assert_eq!(
        gate_diff::drop_trailing_commas(&strs(&["{", "x", ":", "1", ",", "}"])),
        strs(&["{", "x", ":", "1", "}"])
    );
}

#[test]
fn drop_trailing_commas_comma_between_arguments_is_kept() {
    assert_eq!(
        gate_diff::drop_trailing_commas(&strs(&["f", "(", "a", ",", "b", ")"])),
        strs(&["f", "(", "a", ",", "b", ")"])
    );
}

#[test]
fn drop_trailing_commas_trailing_comma_at_the_very_end_of_the_list_is_kept() {
    assert_eq!(
        gate_diff::drop_trailing_commas(&strs(&["a", ","])),
        strs(&["a", ","])
    );
}

fn semantic_ranges_with(
    hunk_spans: &gate_diff::HunkSpans,
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> gate_diff::RangesByFile {
    gate_diff::semantic_ranges(
        hunk_spans,
        |file: &str| old.get(file).cloned().unwrap_or_default(),
        |file: &str| new.get(file).cloned().unwrap_or_default(),
    )
}

#[test]
fn semantic_ranges_comment_only_edit_is_dropped() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((2, 2)), Some((2, 2)))])]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    let x = 1;  // set x\n}\n".to_owned(),
    )]);
    let new = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    let x = 1;\n}\n".to_owned(),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::new()
    );
}

#[test]
fn semantic_ranges_hunk_with_a_real_code_change_is_kept_in_full() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((2, 3)), Some((2, 3)))])]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    // a comment\n    let x = 0;\n}\n".to_owned(),
    )]);
    let new = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    // updated comment\n    let x = 1;\n}\n".to_owned(),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(2, 3)])])
    );
}

#[test]
fn semantic_ranges_only_the_semantically_unchanged_hunk_is_dropped_others_survive() {
    let hunk_spans = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![(Some((2, 2)), Some((2, 2))), (Some((4, 4)), Some((4, 4)))],
    )]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    // comment\n    let x = 1;\n    let y = 1;\n}\n".to_owned(),
    )]);
    let new = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    // different comment\n    let x = 1;\n    let y = 2;\n}\n".to_owned(),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(4, 4)])])
    );
}

#[test]
fn semantic_ranges_file_with_no_surviving_ranges_is_dropped_entirely() {
    let hunk_spans = BTreeMap::from([(
        "src/only_comments.rs".to_owned(),
        vec![(Some((1, 1)), Some((1, 1)))],
    )]);
    let old = BTreeMap::from([(
        "src/only_comments.rs".to_owned(),
        "// nothing but this\n".to_owned(),
    )]);
    let new = BTreeMap::from([(
        "src/only_comments.rs".to_owned(),
        "// nothing but this, reworded\n".to_owned(),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::new()
    );
}

#[test]
fn semantic_ranges_pure_deletion_hunk_contributes_no_range_regardless_of_content() {
    let hunk_spans = BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((5, 7)), None)])]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "fn f() {\n    let x = 1;\n    let y = 2;\n    let z = 3;\n}\n".to_owned(),
    )]);
    let new = BTreeMap::from([("src/lib.rs".to_owned(), "fn f() {\n}\n".to_owned())]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::new()
    );
}

#[test]
fn semantic_ranges_comment_removal_that_collapses_a_block_onto_one_line_is_dropped() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((2, 4)), Some((2, 2)))])]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        [
            "fn f(cond: bool) {",
            "    if cond {",
            "        // Found something",
            "    }",
            "}",
            "",
        ]
        .join("\n"),
    )]);
    let new = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        ["fn f(cond: bool) {", "    if cond {}", "}", ""].join("\n"),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::new()
    );
}

#[test]
fn semantic_ranges_whitespace_reorder_bundled_with_a_real_token_change_is_kept() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 1)), Some((1, 2)))])]);
    let old = BTreeMap::from([("src/lib.rs".to_owned(), "let x = 1;\n".to_owned())]);
    let new = BTreeMap::from([("src/lib.rs".to_owned(), "let   x =\n    2;\n".to_owned())]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 2)])])
    );
}

#[test]
fn semantic_ranges_string_literal_content_is_compared_not_discarded() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 1)), Some((1, 1)))])]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "let s = \"// keep me A\";\n".to_owned(),
    )]);
    let new = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "let s = \"// keep me B\";\n".to_owned(),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 1)])])
    );
}

#[test]
fn semantic_ranges_call_reformatted_onto_fewer_lines_drops_only_its_trailing_comma() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 5)), Some((1, 1)))])]);
    let old = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        [
            "f(",
            "    // pick the material",
            "    material(),",
            "    move || dynamics(),",
            ");",
            "",
        ]
        .join("\n"),
    )]);
    let new = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        "f(material(), move || dynamics());\n".to_owned(),
    )]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::new()
    );
}

#[test]
fn semantic_ranges_trailing_comma_change_bundled_with_a_real_edit_is_still_kept() {
    let hunk_spans =
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(Some((1, 1)), Some((1, 1)))])]);
    let old = BTreeMap::from([("src/lib.rs".to_owned(), "f(a, b,);\n".to_owned())]);
    let new = BTreeMap::from([("src/lib.rs".to_owned(), "f(a, c);\n".to_owned())]);
    assert_eq!(
        semantic_ranges_with(&hunk_spans, &old, &new),
        BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 1)])])
    );
}

#[test]
fn intersects_overlapping_ranges() {
    assert!(gate_diff::intersects((10, 20), (15, 25)));
    assert!(gate_diff::intersects((15, 25), (10, 20)));
}

#[test]
fn intersects_touching_at_a_single_line_counts_as_overlap() {
    assert!(gate_diff::intersects((10, 20), (20, 30)));
}

#[test]
fn intersects_disjoint_ranges() {
    assert!(!gate_diff::intersects((10, 20), (21, 30)));
}

#[test]
fn intersects_one_range_contains_the_other() {
    assert!(gate_diff::intersects((1, 100), (40, 41)));
}

#[test]
fn any_intersect_true_only_when_one_range_matches() {
    let ranges = vec![(1, 5), (50, 60)];
    assert!(gate_diff::any_intersect(&ranges, (55, 58)));
    assert!(!gate_diff::any_intersect(&ranges, (10, 20)));
    assert!(!gate_diff::any_intersect(&[], (1, 1000)));
}

#[test]
fn cargo_bin_dir_honors_cargo_home() {
    let dir = gate_diff::cargo_bin_dir_for(
        Some(std::ffi::OsString::from("/scratch/cargo")),
        PathBuf::from("/unused"),
    );
    assert_eq!(dir, PathBuf::from("/scratch/cargo/bin"));
}

#[test]
fn cargo_bin_dir_falls_back_to_home_cargo() {
    let dir = gate_diff::cargo_bin_dir_for(None, PathBuf::from("/home/test"));
    assert_eq!(dir, PathBuf::from("/home/test/.cargo/bin"));
}

#[test]
fn resolve_cargo_tool_never_installs_when_already_at_the_pinned_location() {
    let root = unique_temp_dir();
    let bin_dir = unique_temp_dir();
    let fake_tool = bin_dir.join("some-tool");
    fs::write(&fake_tool, "#!/bin/sh\n").expect("write fake tool");
    make_executable(&fake_tool).expect("chmod fake tool");

    let installed = std::cell::Cell::new(false);
    let resolved = gate_diff::resolve_cargo_tool_in(&root, &bin_dir, "some-tool", "", |_, _| {
        installed.set(true);
    })
    .expect("resolve succeeds");

    assert!(!installed.get());
    assert_eq!(resolved, fake_tool);
}

#[test]
fn resolve_cargo_tool_installs_when_missing_then_resolves_to_the_pinned_location() {
    let root = unique_temp_dir();
    let bin_dir = unique_temp_dir();
    let fake_tool = bin_dir.join("some-tool");

    let install_count = std::cell::Cell::new(0u32);
    let resolved = gate_diff::resolve_cargo_tool_in(&root, &bin_dir, "some-tool", "", |_, _| {
        install_count.set(install_count.get() + 1);
        fs::write(&fake_tool, "#!/bin/sh\n").expect("write fake tool");
        make_executable(&fake_tool).expect("chmod fake tool");
    })
    .expect("resolve succeeds");

    assert_eq!(install_count.get(), 1);
    assert_eq!(resolved, fake_tool);
}

#[test]
fn resolve_cargo_tool_raises_with_the_hint_when_install_does_not_produce_the_binary() {
    let root = unique_temp_dir();
    let bin_dir = unique_temp_dir();
    let error = gate_diff::resolve_cargo_tool_in(
        &root,
        &bin_dir,
        "some-tool",
        "do not substitute npm",
        |_, _| {},
    )
    .expect_err("install never produces the binary");
    assert!(error.contains("do not substitute npm"));
}

fn git(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git")
}

fn git_ok(args: &[&str], cwd: &Path) -> String {
    let output = git(args, cwd);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn is_shallow(repo: &Path) -> bool {
    git_ok(&["rev-parse", "--is-shallow-repository"], repo) == "true"
}

fn commit_file(repo: &Path, message: &str) -> String {
    fs::write(repo.join("file.txt"), message).expect("write file");
    git_ok(&["add", "."], repo);
    git_ok(&["commit", "--quiet", "-m", message], repo);
    git_ok(&["rev-parse", "HEAD"], repo)
}

fn init_repo(repo: &Path, initial_branch: &str) {
    fs::create_dir_all(repo).expect("create repo dir");
    git_ok(&["init", "--quiet", "-b", initial_branch], repo);
    git_ok(&["config", "user.email", "test@example.com"], repo);
    git_ok(&["config", "user.name", "Test"], repo);
}

#[test]
fn fixture_commits_are_independent_of_user_signing_configuration() {
    let repo = unique_temp_dir();
    init_repo(&repo, "main");
    git_ok(&["config", "commit.gpgsign", "true"], &repo);
    git_ok(
        &["config", "gpg.program", "cranpose-missing-test-signer"],
        &repo,
    );
    let commit = commit_file(&repo, "fixture");
    assert_eq!(git_ok(&["rev-parse", "HEAD"], &repo), commit);
}

fn shallow_checkout_of_branch_tip(origin: &Path, branch: &str, work: &Path) {
    fs::create_dir_all(work).expect("create work dir");
    git_ok(&["init", "--quiet"], work);
    git_ok(
        &[
            "remote",
            "add",
            "origin",
            &format!("file://{}", origin.display()),
        ],
        work,
    );
    git_ok(
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        work,
    );
    let tip = git_ok(&["rev-parse", branch], origin);
    git_ok(
        &[
            "fetch",
            "--quiet",
            "--depth=1",
            "origin",
            &format!("+{tip}:refs/remotes/origin/{branch}"),
        ],
        work,
    );
    git_ok(
        &[
            "checkout",
            "--quiet",
            "-b",
            branch,
            &format!("origin/{branch}"),
        ],
        work,
    );
}

#[test]
fn merge_base_deepens_shallow_history_to_find_the_merge_base() {
    let tmp = unique_temp_dir();
    let origin = tmp.join("origin");
    let work = tmp.join("work");
    init_repo(&origin, "main");
    let shared_ancestor = commit_file(&origin, "shared ancestor");
    git_ok(&["checkout", "--quiet", "-b", "feature"], &origin);
    commit_file(&origin, "feature work");
    git_ok(&["checkout", "--quiet", "main"], &origin);
    commit_file(&origin, "main moved on without the feature branch");

    shallow_checkout_of_branch_tip(&origin, "feature", &work);
    assert!(is_shallow(&work));

    assert_eq!(
        gate_diff::merge_base(&work, "origin/main").expect("merge base found"),
        shared_ancestor
    );
    assert!(!is_shallow(&work));
}

#[test]
fn merge_base_raises_when_histories_truly_share_no_ancestor() {
    let tmp = unique_temp_dir();
    let origin_main = tmp.join("origin_main");
    let origin_feature = tmp.join("origin_feature");
    let work = tmp.join("work");
    init_repo(&origin_main, "main");
    commit_file(&origin_main, "main's own unrelated root");
    init_repo(&origin_feature, "feature");
    commit_file(&origin_feature, "feature's own unrelated root");

    fs::create_dir_all(&work).expect("create work dir");
    git_ok(&["init", "--quiet"], &work);
    git_ok(
        &[
            "remote",
            "add",
            "origin",
            &format!("file://{}", origin_main.display()),
        ],
        &work,
    );
    git_ok(&["fetch", "--quiet", "origin", "main"], &work);
    git_ok(
        &[
            "remote",
            "add",
            "elsewhere",
            &format!("file://{}", origin_feature.display()),
        ],
        &work,
    );
    git_ok(&["fetch", "--quiet", "elsewhere", "feature"], &work);
    git_ok(
        &["checkout", "--quiet", "-b", "feature", "elsewhere/feature"],
        &work,
    );
    assert!(!is_shallow(&work));

    let error = gate_diff::merge_base(&work, "origin/main").expect_err("no shared history");
    assert!(error.contains("share no common ancestor"));
}

const OVER_LIMIT_FUNCTION: &str = "fn deeply_branching(x: i32) -> i32 {\n    if x == 0 { return 0; }\n    if x == 1 { return 1; }\n    if x == 2 { return 2; }\n    if x == 3 { return 3; }\n    if x == 4 { return 4; }\n    if x == 5 { return 5; }\n    if x == 6 { return 6; }\n    if x == 7 { return 7; }\n    if x == 8 { return 8; }\n    if x == 9 { return 9; }\n    x\n}";

fn init_repo_with_base_commit(repo: &Path) {
    fs::create_dir_all(repo.join("src")).expect("create src dir");
    git_ok(&["init", "--quiet", "-b", "main"], repo);
    git_ok(&["config", "user.email", "test@example.com"], repo);
    git_ok(&["config", "user.name", "Test"], repo);
    fs::write(repo.join("src/lib.rs"), format!("{OVER_LIMIT_FUNCTION}\n")).expect("write lib.rs");
    git_ok(&["add", "."], repo);
    git_ok(&["commit", "--quiet", "-m", "base"], repo);
}

fn write_and_commit(repo: &Path, contents: &str, message: &str) {
    fs::write(repo.join("src/lib.rs"), contents).expect("write lib.rs");
    git_ok(&["add", "."], repo);
    git_ok(&["commit", "--quiet", "-m", message], repo);
}

#[test]
fn changed_ranges_removing_a_comment_inside_the_function_does_not_touch_it() {
    let tmp = unique_temp_dir();
    let repo = tmp.join("repo");
    init_repo_with_base_commit(&repo);

    let with_comment = OVER_LIMIT_FUNCTION.replace(
        "    x\n}",
        "    // fall through for anything else\n    x\n}",
    );
    write_and_commit(&repo, &format!("{with_comment}\n"), "add a comment");

    let comment_removed = with_comment.replace("    // fall through for anything else\n", "");
    write_and_commit(
        &repo,
        &format!("{comment_removed}\n"),
        "remove only the comment",
    );

    let ranges =
        gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
    assert_eq!(
        ranges,
        BTreeMap::new(),
        "a hunk that only deleted a comment line must not touch anything"
    );
}

#[test]
fn changed_ranges_a_genuine_logic_edit_in_the_same_function_still_touches_it() {
    let tmp = unique_temp_dir();
    let repo = tmp.join("repo");
    init_repo_with_base_commit(&repo);

    let edited = OVER_LIMIT_FUNCTION.replace(
        "    if x == 9 { return 9; }",
        "    if x == 9 { return 90; }",
    );
    write_and_commit(&repo, &format!("{edited}\n"), "change a return value");

    let ranges =
        gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
    let touched = ranges.get("src/lib.rs").expect("src/lib.rs touched");
    assert!(
        gate_diff::any_intersect(touched, (1, 13)),
        "a real logic edit must still land inside the function's span, got {touched:?}"
    );
}

#[test]
fn changed_ranges_mixed_hunk_of_comment_and_logic_still_touches_it() {
    let tmp = unique_temp_dir();
    let repo = tmp.join("repo");
    init_repo_with_base_commit(&repo);

    let edited = OVER_LIMIT_FUNCTION.replace(
        "    x\n}",
        "    // fall through for anything else\n    x + 1\n}",
    );
    write_and_commit(
        &repo,
        &format!("{edited}\n"),
        "comment plus a real edit, one hunk",
    );

    let ranges =
        gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
    assert!(gate_diff::any_intersect(
        ranges.get("src/lib.rs").expect("touched"),
        (1, 13)
    ));
}

#[test]
fn changed_ranges_comment_deletion_that_collapses_a_branch_onto_one_line_does_not_touch_it() {
    let tmp = unique_temp_dir();
    let repo = tmp.join("repo");
    init_repo_with_base_commit(&repo);

    let with_comment_block = OVER_LIMIT_FUNCTION.replace(
        "    if x == 9 { return 9; }",
        "    if x == 9 {\n        // nothing special about nine\n    }",
    );
    write_and_commit(
        &repo,
        &format!("{with_comment_block}\n"),
        "expand nine's branch",
    );

    let collapsed = OVER_LIMIT_FUNCTION.replace("    if x == 9 { return 9; }", "    if x == 9 {}");
    write_and_commit(
        &repo,
        &format!("{collapsed}\n"),
        "delete the comment, collapse the block",
    );

    let ranges =
        gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
    assert_eq!(
        ranges,
        BTreeMap::new(),
        "deleting a comment and collapsing its now-empty block is not a logic edit, got {ranges:?}"
    );
}

#[test]
fn changed_ranges_reformatting_a_line_while_also_changing_it_still_touches_it() {
    let tmp = unique_temp_dir();
    let repo = tmp.join("repo");
    init_repo_with_base_commit(&repo);

    let edited = OVER_LIMIT_FUNCTION.replace(
        "    if x == 9 { return 9; }",
        "    if x == 9 {\n        return 90;\n    }",
    );
    write_and_commit(
        &repo,
        &format!("{edited}\n"),
        "reformat the branch onto three lines and change its value",
    );

    let ranges =
        gate_diff::changed_ranges(&repo, "HEAD~1", "*.rs").expect("changed_ranges succeeds");
    let touched = ranges.get("src/lib.rs").expect("src/lib.rs touched");
    assert!(
        gate_diff::any_intersect(touched, (1, 13)),
        "a real value change bundled with a reformat must not be normalized away, got {touched:?}"
    );
}

fn func_space(
    name: Option<&str>,
    start: u64,
    end: u64,
    cyclomatic: f64,
    children: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "kind": "function",
        "name": name,
        "start_line": start,
        "end_line": end,
        "metrics": {"cyclomatic": {"sum": cyclomatic}},
        "spaces": children,
    })
}

#[test]
fn functions_in_named_function_with_no_closures_is_reported_once() {
    let mut out = Vec::new();
    complexity_gate::functions_in(&func_space(Some("f"), 1, 10, 5.0, vec![]), &mut out, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "f");
}

#[test]
fn functions_in_closure_nested_in_a_named_function_is_not_reported_separately() {
    let closure = func_space(None, 2, 9, 25.0, vec![]);
    let outer = func_space(Some("main"), 1, 10, 27.0, vec![closure]);
    let mut out = Vec::new();
    complexity_gate::functions_in(&outer, &mut out, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "main");
    assert_eq!(out[0].cyclomatic, Some(27));
}

#[test]
fn functions_in_closure_nested_in_a_closure_still_collapses_to_one_report() {
    let inner_closure = func_space(None, 3, 8, 10.0, vec![]);
    let outer_closure = func_space(None, 2, 9, 15.0, vec![inner_closure]);
    let outer = func_space(Some("run"), 1, 10, 20.0, vec![outer_closure]);
    let mut out = Vec::new();
    complexity_gate::functions_in(&outer, &mut out, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "run");
}

#[test]
fn functions_in_named_function_nested_inside_a_closure_is_still_reported() {
    let nested_fn = func_space(Some("helper"), 3, 5, 8.0, vec![]);
    let closure = func_space(None, 2, 6, 9.0, vec![nested_fn]);
    let outer = func_space(Some("run"), 1, 7, 12.0, vec![closure]);
    let mut out = Vec::new();
    complexity_gate::functions_in(&outer, &mut out, false);
    let names: BTreeSet<_> = out.iter().map(|f| f.name.clone()).collect();
    assert_eq!(
        names,
        BTreeSet::from(["run".to_owned(), "helper".to_owned()])
    );
}

#[test]
fn functions_in_top_level_closure_with_no_enclosing_function_is_still_reported() {
    let top_level_closure = func_space(None, 1, 5, 12.0, vec![]);
    let mut out = Vec::new();
    complexity_gate::functions_in(&top_level_closure, &mut out, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "<anonymous>");
}

#[test]
fn functions_in_rust_code_analysis_cli_names_a_closure_the_literal_string_anonymous() {
    let closure = func_space(Some("<anonymous>"), 2, 9, 25.0, vec![]);
    let outer = func_space(Some("main"), 1, 10, 27.0, vec![closure]);
    let mut out = Vec::new();
    complexity_gate::functions_in(&outer, &mut out, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "main");
}

#[test]
fn functions_in_sibling_functions_are_both_reported() {
    let root = serde_json::json!({
        "kind": "file",
        "spaces": [func_space(Some("a"), 1, 5, 5.0, vec![]), func_space(Some("b"), 10, 15, 5.0, vec![])],
    });
    let mut out = Vec::new();
    complexity_gate::functions_in(&root, &mut out, false);
    let names: BTreeSet<_> = out.iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, BTreeSet::from(["a".to_owned(), "b".to_owned()]));
}

fn function_metric(
    name: &str,
    start: usize,
    end: usize,
    cyclomatic: i64,
) -> complexity_gate::FunctionMetric {
    complexity_gate::FunctionMetric {
        name: name.to_owned(),
        start: Some(start),
        end: Some(end),
        cyclomatic: Some(cyclomatic),
    }
}

#[test]
fn complexity_find_violations_flags_only_functions_the_diff_touches() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(10, 15)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![
            function_metric("touched_and_complex", 10, 15, 25),
            function_metric("untouched_and_complex", 100, 120, 99),
            function_metric("touched_but_simple", 12, 13, 3),
        ],
    )]);
    let violations =
        complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("touched_and_complex"));
    assert!(violations[0].contains("is new at 25"));
}

#[test]
fn complexity_find_violations_no_violations_when_nothing_over_the_limit() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 100)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("fine", 1, 10, 5)],
    )]);
    assert_eq!(
        complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20),
        Vec::<String>::new()
    );
}

#[test]
fn complexity_find_violations_untouched_file_contributes_no_violations() {
    let ranges = BTreeMap::from([("src/other.rs".to_owned(), vec![(1, 5)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("huge", 1, 500, 500)],
    )]);
    assert_eq!(
        complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20),
        Vec::<String>::new()
    );
}

#[test]
fn complexity_find_violations_already_over_limit_function_untouched_by_a_real_edit_passes() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 300)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("run", 1, 300, 174)],
    )]);
    let old_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("run", 1, 305, 174)],
    )]);
    assert_eq!(
        complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20),
        Vec::<String>::new()
    );
}

#[test]
fn complexity_find_violations_already_over_limit_function_made_simpler_passes() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 300)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("run", 1, 300, 150)],
    )]);
    let old_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("run", 1, 305, 174)],
    )]);
    assert_eq!(
        complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20),
        Vec::<String>::new()
    );
}

#[test]
fn complexity_find_violations_already_over_limit_function_made_worse_still_trips_the_gate() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 300)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("run", 1, 300, 180)],
    )]);
    let old_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("run", 1, 305, 174)],
    )]);
    let violations = complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("was 174, is now 180"));
}

#[test]
fn complexity_find_violations_under_the_limit_before_and_over_after_still_trips_the_gate() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 20)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("f", 1, 20, 25)],
    )]);
    let old_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("f", 1, 18, 18)],
    )]);
    let violations = complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("was 18, is now 25"));
}

#[test]
fn complexity_find_violations_new_function_with_no_old_counterpart_is_judged_against_the_limit() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 20)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("brand_new", 1, 20, 25)],
    )]);
    let violations =
        complexity_gate::find_violations(&ranges, &new_functions, &BTreeMap::new(), 20);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("is new at 25"));
}

#[test]
fn complexity_find_violations_same_named_functions_are_matched_by_occurrence_order() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 5), (10, 15)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![
            function_metric("new", 1, 5, 22),
            function_metric("new", 10, 15, 30),
        ],
    )]);
    let old_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![
            function_metric("new", 1, 5, 22),
            function_metric("new", 9, 14, 18),
        ],
    )]);
    let violations = complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains(":10-15"));
    assert!(violations[0].contains("was 18, is now 30"));
}

#[test]
fn complexity_find_violations_anonymous_function_has_no_old_counterpart_even_if_old_side_has_one() {
    let ranges = BTreeMap::from([("src/lib.rs".to_owned(), vec![(1, 5)])]);
    let new_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("<anonymous>", 1, 5, 25)],
    )]);
    let old_functions = BTreeMap::from([(
        "src/lib.rs".to_owned(),
        vec![function_metric("<anonymous>", 1, 5, 99)],
    )]);
    let violations = complexity_gate::find_violations(&ranges, &new_functions, &old_functions, 20);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("is new at 25"));
}

#[test]
fn load_max_cyclomatic_reads_the_configured_limit() {
    let dir = unique_temp_dir();
    let config_path = dir.join("code_quality_gates.toml");
    fs::write(&config_path, "[complexity]\nmax_cyclomatic = 20\n").expect("write config");
    assert_eq!(
        complexity_gate::load_max_cyclomatic(&config_path).expect("load succeeds"),
        20
    );
}

#[test]
fn load_max_cyclomatic_fails_when_the_key_is_missing() {
    let dir = unique_temp_dir();
    let config_path = dir.join("code_quality_gates.toml");
    fs::write(&config_path, "[complexity]\n").expect("write config");
    assert!(complexity_gate::load_max_cyclomatic(&config_path).is_err());
}

fn duplicate(
    first: (&str, usize, usize),
    second: (&str, usize, usize),
    lines: usize,
) -> duplication_gate::Duplicate {
    duplication_gate::Duplicate {
        first_file: duplication_gate::DuplicateSide {
            name: first.0.to_owned(),
            start: first.1,
            end: first.2,
        },
        second_file: duplication_gate::DuplicateSide {
            name: second.0.to_owned(),
            start: second.1,
            end: second.2,
        },
        lines,
        fragment: String::new(),
    }
}

fn duplicate_of(
    first: (&str, usize, usize),
    second: (&str, usize, usize),
    lines: usize,
    fragment: &str,
) -> duplication_gate::Duplicate {
    duplication_gate::Duplicate {
        fragment: fragment.to_owned(),
        ..duplicate(first, second, lines)
    }
}

#[test]
fn duplication_find_violations_clone_that_moved_with_its_code_passes() {
    let ranges = BTreeMap::from([
        ("src/tests/a_tests.rs".to_owned(), vec![(1, 40)]),
        ("src/a.rs".to_owned(), vec![(300, 340)]),
    ]);
    let moved = duplicate_of(
        ("src/tests/a_tests.rs", 1, 12),
        ("src/tests/a_tests.rs", 20, 31),
        12,
        "let x = 1;\nlet y = 2;\n",
    );
    let old_source = duplication_gate::clone_text(
        "    /// The value.\n    let x =\n        1;\n    // and the other\n    let y = 2;\n",
    );
    assert_eq!(
        duplication_gate::find_violations(
            std::slice::from_ref(&moved),
            &[],
            &[old_source],
            &ranges
        ),
        Vec::<String>::new()
    );
    let other = duplication_gate::clone_text("    let z = 3;\n");
    assert_eq!(
        duplication_gate::find_violations(&[moved], &[], &[other], &ranges).len(),
        1
    );
}

#[test]
fn duplication_find_violations_clone_rustfmt_rejoined_after_moving_passes() {
    let ranges = BTreeMap::from([("src/tests/a_tests.rs".to_owned(), vec![(1, 40)])]);
    let moved = duplicate_of(
        ("src/tests/a_tests.rs", 1, 12),
        ("src/tests/a_tests.rs", 20, 31),
        12,
        "fn layout(&self, text: &Text) -> Layout {\n    panic!(\"unused\");\n}\n",
    );
    let old_source = duplication_gate::clone_text(
        "    fn layout(\n        &self,\n        text: &Text,\n    ) -> Layout {\n        panic!(\"unused\");\n    }\n",
    );
    assert_eq!(
        duplication_gate::find_violations(
            std::slice::from_ref(&moved),
            &[],
            std::slice::from_ref(&old_source),
            &ranges
        ),
        Vec::<String>::new()
    );
    let cut_inside_a_list = duplicate_of(
        ("src/tests/a_tests.rs", 1, 12),
        ("src/tests/a_tests.rs", 20, 31),
        12,
        "fn layout(\n    &self,\n    text: &Text,\n",
    );
    assert_eq!(
        duplication_gate::find_violations(&[cut_inside_a_list], &[], &[old_source], &ranges),
        Vec::<String>::new()
    );
}

#[test]
fn duplication_find_violations_new_code_duplicating_old_code_fails() {
    let ranges = BTreeMap::from([("src/new.rs".to_owned(), vec![(1, 20)])]);
    let dup = duplicate(("src/new.rs", 5, 16), ("src/old.rs", 100, 111), 12);
    let violations = duplication_gate::find_violations(&[dup], &[], &[], &ranges);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("src/new.rs:5-16 (new)"));
    assert!(!violations[0].contains("src/old.rs:100-111 (new)"));
}

#[test]
fn duplication_find_violations_two_untouched_clones_are_not_flagged() {
    let ranges = BTreeMap::from([("src/elsewhere.rs".to_owned(), vec![(1, 5)])]);
    let dup = duplicate(("src/old_a.rs", 1, 12), ("src/old_b.rs", 1, 12), 12);
    assert_eq!(
        duplication_gate::find_violations(&[dup], &[], &[], &ranges),
        Vec::<String>::new()
    );
}

#[test]
fn duplication_find_violations_new_code_duplicating_itself_flags_both_sides() {
    let ranges = BTreeMap::from([("src/new.rs".to_owned(), vec![(1, 50)])]);
    let dup = duplicate(("src/new.rs", 1, 12), ("src/new.rs", 20, 31), 12);
    let violations = duplication_gate::find_violations(&[dup], &[], &[], &ranges);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("(new)"));
}

#[test]
fn duplication_find_violations_touched_clone_already_duplicated_before_passes() {
    let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
    let new_dup = duplicate(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), 12);
    let old_dup = duplicate(("src/a.rs", 9, 20), ("src/b.rs", 38, 49), 12);
    assert_eq!(
        duplication_gate::find_violations(&[new_dup], &[old_dup], &[], &ranges),
        Vec::<String>::new()
    );
}

#[test]
fn duplication_find_violations_touched_clone_with_no_old_counterpart_still_fails() {
    let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
    let new_dup = duplicate(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), 12);
    let violations = duplication_gate::find_violations(&[new_dup], &[], &[], &ranges);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("introduced by this diff"));
}

#[test]
fn duplication_find_violations_unrelated_old_clone_between_the_same_files_grants_no_amnesty() {
    let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
    let new_dup = duplicate(("src/a.rs", 10, 21), ("src/b.rs", 40, 51), 12);
    let unrelated_old_dup = duplicate(("src/a.rs", 200, 299), ("src/b.rs", 300, 399), 100);
    let violations =
        duplication_gate::find_violations(&[new_dup], &[unrelated_old_dup], &[], &ranges);
    assert_eq!(violations.len(), 1);
}

#[test]
fn duplication_find_violations_reformatted_clone_within_size_tolerance_still_matches() {
    let ranges = BTreeMap::from([("src/a.rs".to_owned(), vec![(10, 12)])]);
    let new_dup = duplicate(("src/a.rs", 10, 19), ("src/b.rs", 40, 49), 10);
    let old_dup = duplicate(("src/a.rs", 9, 20), ("src/b.rs", 38, 49), 12);
    assert_eq!(
        duplication_gate::find_violations(&[new_dup], &[old_dup], &[], &ranges),
        Vec::<String>::new()
    );
}

#[test]
fn file_pair_order_independent() {
    let a = duplicate(("x.rs", 0, 0), ("y.rs", 0, 0), 0);
    let b = duplicate(("y.rs", 0, 0), ("x.rs", 0, 0), 0);
    assert_eq!(
        duplication_gate::file_pair(&a),
        duplication_gate::file_pair(&b)
    );
}

#[test]
fn already_duplicated_before_no_old_duplicates_at_all() {
    let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
    assert!(!duplication_gate::already_duplicated_before(
        &candidate,
        &[]
    ));
}

#[test]
fn already_duplicated_before_matching_pair_within_size_tolerance() {
    let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
    let old = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 10);
    assert!(duplication_gate::already_duplicated_before(
        &candidate,
        &[old]
    ));
}

#[test]
fn already_duplicated_before_matching_pair_outside_size_tolerance_does_not_count() {
    let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
    let old = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 100);
    assert!(!duplication_gate::already_duplicated_before(
        &candidate,
        &[old]
    ));
}

#[test]
fn already_duplicated_before_different_pair_does_not_count() {
    let candidate = duplicate(("a.rs", 1, 1), ("b.rs", 1, 1), 12);
    let old = duplicate(("a.rs", 1, 1), ("c.rs", 1, 1), 12);
    assert!(!duplication_gate::already_duplicated_before(
        &candidate,
        &[old]
    ));
}

#[test]
fn load_duplication_config_reads_lines_tokens_and_ignore_globs() {
    let dir = unique_temp_dir();
    let config_path = dir.join("code_quality_gates.toml");
    fs::write(
        &config_path,
        "[duplication]\nmin_lines = 10\nmin_tokens = 50\nignore_globs = [\"**/target/**\"]\n",
    )
    .expect("write config");
    let config = duplication_gate::load_duplication_config(&config_path).expect("load succeeds");
    assert_eq!(config.min_lines, 10);
    assert_eq!(config.min_tokens, 50);
    assert_eq!(config.ignore_globs, vec!["**/target/**".to_owned()]);
}

#[test]
fn load_duplication_config_defaults_ignore_globs_to_empty() {
    let dir = unique_temp_dir();
    let config_path = dir.join("code_quality_gates.toml");
    fs::write(
        &config_path,
        "[duplication]\nmin_lines = 10\nmin_tokens = 50\n",
    )
    .expect("write config");
    let config = duplication_gate::load_duplication_config(&config_path).expect("load succeeds");
    assert_eq!(config.ignore_globs, Vec::<String>::new());
}

#[test]
fn parse_duplicates_reads_first_and_second_file_spans() {
    let report = serde_json::json!({
        "duplicates": [
            {
                "firstFile": {"name": "a.rs", "start": 1, "end": 12},
                "secondFile": {"name": "b.rs", "start": 5, "end": 16},
                "lines": 12,
            }
        ]
    });
    let duplicates = duplication_gate::parse_duplicates(&report);
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].first_file.name, "a.rs");
    assert_eq!(duplicates[0].lines, 12);
}

#[test]
fn parse_duplicates_skips_entries_missing_required_fields() {
    let report =
        serde_json::json!({"duplicates": [{"firstFile": {"name": "a.rs", "start": 1, "end": 12}}]});
    assert_eq!(
        duplication_gate::parse_duplicates(&report),
        Vec::<duplication_gate::Duplicate>::new()
    );
}

#[test]
fn parse_duplicates_defaults_to_empty_when_the_key_is_absent() {
    let report = serde_json::json!({});
    assert_eq!(
        duplication_gate::parse_duplicates(&report),
        Vec::<duplication_gate::Duplicate>::new()
    );
}

#[test]
fn duplication_involved_files_collects_both_sides_of_every_candidate() {
    let a = duplicate(("a.rs", 1, 5), ("b.rs", 1, 5), 5);
    let b = duplicate(("b.rs", 10, 15), ("c.rs", 10, 15), 5);
    assert_eq!(
        duplication_gate::involved_files(&[a, b]),
        vec!["a.rs".to_owned(), "b.rs".to_owned(), "c.rs".to_owned()]
    );
}

#[test]
fn parse_gate_options_defaults() {
    let options = GateOptions::parse(&[], "complexity-gate").expect("parse succeeds");
    assert_eq!(options.base, "origin/main");
    assert_eq!(options.config, None);
}

#[test]
fn parse_gate_options_explicit_base_and_config() {
    let options = GateOptions::parse(
        &[
            "--base".into(),
            "main".into(),
            "--config".into(),
            "gates.toml".into(),
        ],
        "duplication-gate",
    )
    .expect("parse succeeds");
    assert_eq!(options.base, "main");
    assert_eq!(options.config, Some(PathBuf::from("gates.toml")));
}

#[test]
fn parse_gate_options_rejects_unknown_option() {
    let error = GateOptions::parse(&["--nope".into()], "complexity-gate")
        .expect_err("rejects unknown option");
    assert!(error.contains("complexity-gate"));
    assert!(error.contains("--nope"));
}

#[test]
fn violations_message_lists_each_violation_indented() {
    let message = violations_message(
        "complexity-gate: 2 function(s) over the limit:".to_owned(),
        &[
            "a.rs:1-2 f is new at 21 (limit 20)".to_owned(),
            "b.rs:3-4 g is new at 30 (limit 20)".to_owned(),
        ],
    );
    assert_eq!(
        message,
        "complexity-gate: 2 function(s) over the limit:\n  a.rs:1-2 f is new at 21 (limit 20)\n  b.rs:3-4 g is new at 30 (limit 20)"
    );
}

#[path = "release_tests.rs"]
mod release;

fn unique_temp_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/test-output/xtask");
    fs::create_dir_all(&root).expect("create fixture root");
    tempfile::Builder::new()
        .prefix("cranpose-xtask-test-")
        .tempdir_in(root)
        .expect("reserve a unique fixture directory")
        .keep()
}
