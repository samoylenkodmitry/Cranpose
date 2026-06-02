# Dependency Alignment

This note records duplicate-version dependency alignment after the feature split, text-stack cleanup, size-budget work, and WGPU-stack alignment.

## Current Budget

`cargo xtask dependency-budget` now has an empty duplicate-version allowlist. Use `cargo xtask dependency-budget --explain` to print the duplicate root versions, their direct owners, and the slice status for each checked scope. The same gate also rejects `cranpose/renderer-pixels` if it pulls the external `pixels` crate or WGPU renderer packages. `cargo xtask dependency-budget --strict` is the full zero duplicate-version gate; repeated roots at the same package version stay diagnostic-only. `cargo xtask dependency-budget --strict --slice wgpu-stack` proves the WGPU-owned version splits remain gone.

- Workspace: none.
- Workspace all-features: none.

The gate is useful because any new duplicate-version family now fails CI instead of being absorbed into an allowlist.

## Evidence

`foldhash` and `hashbrown` were owned by the WGPU stack and adjacent tooling:

- `foldhash 0.1` and `hashbrown 0.15` come through `gpu-descriptor -> wgpu-hal`.
- `foldhash 0.2` and `hashbrown 0.16` come through `gpu-allocator`, `indexmap`, `naga`, `wgpu`, `wgpu-core`, `wgpu-hal`, and proc-macro tooling.

The WGPU-stack split is now aligned by patching `gpu-descriptor 0.3.2` to the upstream commit `79804e422186805f1ff5ab3d8310c07c145a6731`, which updates its `hashbrown` dependency to `0.16`. Upstream `master` already moved on to `hashbrown 0.17`, so this exact commit is intentionally pinned until an upstream release aligns with the rest of WGPU 29.

`rustc-hash` is no longer an active duplicate-version budget root. Stale lockfile package entries are not counted by the dependency-budget gate.

The inverse-tree probes captured in `dep_inverse_*.tmp` show that the first
slice is not a single direct dependency problem:

- Before the alignment patch, `hashbrown 0.15` and `foldhash 0.1` entered through `gpu-descriptor -> wgpu-hal`.
- After the alignment patch, `gpu-descriptor`, WGPU internals, `gpu-allocator`, `indexmap 2.13`, and proc-macro tooling share `hashbrown 0.16` and `foldhash 0.2`.
- Cranpose-owned crates no longer depend on `rustc-hash` directly; core collection aliases use the existing `ahash` dependency.

`tiny-skia` and `tiny-skia-path` are aligned in normal workspace and
all-features builds:

- `tiny-skia 0.11` comes from `sctk-adwaita -> winit-wayland -> winit` and
  Cranpose-owned rasterization code.

The all-features-only duplicate-version additions have been removed. `serde`
and `serde_core` may still appear as repeated roots at the same package version;
those are diagnostic-only and do not represent duplicate semver roots.

## Decisions

Local direct dependency ownership:

- Keep Cranpose-owned collection aliases on `ahash`. Switching local code to WGPU-internal hashing does not remove the WGPU-owned `hashbrown`/`foldhash` split.
- Keep `tiny-skia 0.11.4` for Cranpose render/common code. The software text
  rasterizer compiles against the same tiny-skia line as the current
  `sctk-adwaita -> winit` platform stack, with PNG decoding disabled because it
  only rasterizes paths.
- Keep software text font, measurement, layout, cursor mapping, and rasterization
  ownership in `cranpose-render-common`. `cranpose-render-pixels` now depends on
  the common text backend instead of owning `ab_glyph` directly; WGPU text
  rendering uses the same in-tree software backend.
- Keep the in-tree pixels renderer independent from the external `pixels` crate.
  `cranpose/renderer-pixels` now enables only `cranpose-render-pixels`; it does
  not pull `pixels -> wgpu` default features into all-features builds.
- Keep SVG rasterization local to `cranpose-ui` and backed by the workspace-aligned `tiny-skia 0.11.4` line. The public `SvgPainter` behavior remains behind the `svg` feature, while the implementation no longer pulls `resvg/usvg/roxmltree` or a second tiny-skia line into all-features.
- Keep native system-theme detection in `cranpose-services` dependency-free. It uses platform settings commands when `system-theme` is enabled and falls back to `Light`; this preserves the service API without pulling portal async stacks into all-features.
- Keep Vulkan enabled for native WGPU. Disabling Vulkan clears the duplicate
  graph in a dependency probe, but `robot_renderer_micro_contract` fails on the
  Linux/X11 test host with no compatible WGPU adapter because the GL path cannot
  present to the provided surface.
- Keep WGPU backend features target-specific. Linux and Android use GLES/Vulkan,
  Windows uses DX12, macOS uses Metal, and wasm uses WebGPU/WebGL. This removes
  the broad desktop backend feature bundle without changing the Linux renderer
  contract that still requires Vulkan.
- Keep the `gpu-descriptor` crates.io patch pinned to upstream commit
  `79804e422186805f1ff5ab3d8310c07c145a6731`. This is the upstream
  `hashbrown 0.16` alignment commit; newer upstream `master` currently uses
  `hashbrown 0.17` and would reintroduce a duplicate-version family.

Future version-change candidates:

- Check whether a newer `winit`/`sctk-adwaita` stack aligns `tiny-skia` with the renderer stack.
- Check whether a newer WGPU line or `gpu-descriptor` crates.io release aligns `hashbrown` or `foldhash` without the patch.
- Leave `zip` unchanged for this slice. With `indexmap 2.13.0`, it shares the
  current `hashbrown 0.16` root instead of owning a separate `hashbrown 0.17`
  root.
- Leave the SVG path on the in-crate parser/rasterizer unless full SVG coverage is explicitly required; adding a general-purpose SVG library must preserve the strict optional-features budget.

Each candidate changes library versions or library selection, so it must run through the dependency-change rule: inspect `cargo tree --duplicates`, inspect inverse trees for affected packages, apply one focused change, then run the full validation gate.

## Resolver Probe

Current non-mutating resolver probes are captured in repo-local logs:

- `cargo_info_*.tmp` registry probes show the current selected/latest versions:
  `wgpu 29.0.3`, `winit 0.31.0-beta.2`, `sctk-adwaita 0.11.0`,
  `gpu-descriptor 0.3.2`, `gpu-allocator 0.28.0`, `indexmap 2.13.0`,
  `resvg/usvg 0.47.0`, and `dark-light 2.0.0`.
- Current refresh logs `cargo_info_wgpu_current_refresh2.tmp`,
  `cargo_info_gpu_descriptor_current_refresh2.tmp`,
  `dep_inverse_gpu_descriptor_current.tmp`, and
  `dep_inverse_hashbrown_015_current.tmp` confirm the remaining
  `hashbrown 0.15` root is owned by `gpu-descriptor 0.3.2 -> wgpu-hal
  29.0.3`, with `wgpu 29.0.3` still the selected current WGPU line.
- Registry probes `cargo_search_wgpu_current2.tmp`,
  `cargo_search_gpu_descriptor_current.tmp`, `cargo_info_wgpu_30_probe.tmp`,
  and `cargo_info_gpu_descriptor_04_probe.tmp` confirm there is no released
  `wgpu 30.0.0` or `gpu-descriptor 0.4.0` candidate on crates.io today. The
  current crates.io releases remain `wgpu 29.0.3` and `gpu-descriptor 0.3.2`.
- Fresh registry probes on 2026-05-25 are captured in
  `cargo_search_wgpu_current3.tmp`, `cargo_search_gpu_descriptor_current2.tmp`,
  `cargo_info_wgpu_current3.tmp`, and `cargo_info_gpu_descriptor_current3.tmp`.
  They still show `wgpu 29.0.3` and `gpu-descriptor 0.3.2` as the current
  released crates, so there is no upstream WGPU-stack alignment release to apply
  for the remaining `foldhash`/`hashbrown` split.
- Fresh registry probes on 2026-05-26 are captured in
  `cargo_search_wgpu_after_presented_geometry.tmp`,
  `cargo_search_gpu_descriptor_after_presented_geometry.tmp`,
  `cargo_info_wgpu_after_presented_geometry.tmp`, and
  `cargo_info_gpu_descriptor_after_presented_geometry.tmp`. They still show
  `wgpu 29.0.3` and `gpu-descriptor 0.3.2` as the current released crates.
  `dependency_budget_wgpu_strict_after_presented_geometry.tmp` still fails only
  on WGPU-stack `foldhash` and `hashbrown`.
- The dependency refresh after the lazy diagnostics cleanup is captured in
  `cargo_search_wgpu_lazy_diag.tmp`, `cargo_search_gpu_descriptor_lazy_diag.tmp`,
  `cargo_info_wgpu_lazy_diag.tmp`, `cargo_info_gpu_descriptor_lazy_diag.tmp`,
  `dep_update_probe_wgpu_stack_lazy_diag.tmp`,
  `dependency_budget_wgpu_strict_lazy_diag.tmp`, and
  `dependency_budget_after_lazy_diag.tmp`. The latest released crates remain
  `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack update locks
  zero packages; normal dependency budget passes; strict WGPU-stack budget still
  fails only `foldhash` and `hashbrown`.
- The roadmap-continuation refresh is captured in
  `cargo_search_wgpu_roadmap_continue.tmp`,
  `cargo_search_gpu_descriptor_roadmap_continue.tmp`,
  `dependency_budget_roadmap_continue.tmp`, and
  `dependency_budget_wgpu_strict_roadmap_continue.tmp`. Crates.io still reports
  `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; normal dependency budget passes;
  strict WGPU-stack budget still fails only `foldhash` and `hashbrown`.
- The 2026-05-26 goal-continuation refresh is captured in
  `cargo_search_wgpu_goal_continue.tmp`,
  `cargo_search_gpu_descriptor_goal_continue.tmp`,
  `cargo_info_wgpu_goal_continue.tmp`,
  `cargo_info_gpu_descriptor_goal_continue.tmp`,
  `dep_update_probe_wgpu_stack_goal_continue.tmp`,
  `dep_inverse_hashbrown_015_goal_continue.tmp`,
  `dep_inverse_hashbrown_016_goal_continue.tmp`,
  `dep_inverse_foldhash_01_goal_continue.tmp`,
  `dependency_budget_after_state_text_poison_x11.tmp`, and
  `dependency_budget_wgpu_strict_after_svg_density_cache_cleanup.tmp`. Crates.io
  still reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run
  WGPU-stack update locks zero packages; normal dependency budget passes;
  strict WGPU-stack budget still fails only `foldhash` and `hashbrown`.
- The modifier/pixels cleanup refresh is captured in
  `cargo_search_wgpu_after_modifier_pixels.tmp`,
  `cargo_search_gpu_descriptor_after_modifier_pixels.tmp`,
  `cargo_info_wgpu_after_modifier_pixels.tmp`,
  `cargo_info_gpu_descriptor_after_modifier_pixels.tmp`,
  `dep_update_probe_wgpu_stack_after_modifier_pixels.tmp`,
  `dependency_budget_after_modifier_pixels.tmp`, and
  `dependency_budget_wgpu_strict_after_modifier_pixels.tmp`. Crates.io still
  reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack
  update locks zero packages; normal dependency budget passes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`.
- The X11 bounds/snapshot-initialization refresh is captured in
  `dependency_budget_after_x11_bounds_snapshot_init.tmp` and
  `dependency_budget_wgpu_strict_after_x11_bounds_snapshot_init.tmp`. Normal
  dependency budget still passes with only WGPU-stack `foldhash` and
  `hashbrown`; strict WGPU-stack budget still fails only those two families in
  normal workspace and all-features scopes.
- The LeetCodeDaily content-bounds refresh is captured in
  `dependency_budget_wgpu_strict_after_leetcodedaily_bounds.tmp`,
  `cargo_tree_duplicates_after_leetcodedaily_bounds.tmp`,
  `dep_inverse_hashbrown_015_after_leetcodedaily_bounds.tmp`,
  `dep_inverse_hashbrown_016_after_leetcodedaily_bounds.tmp`,
  `dep_inverse_foldhash_01_after_leetcodedaily_bounds.tmp`, and
  `dep_inverse_foldhash_02_after_leetcodedaily_bounds.tmp`. Strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`; the inverse trees still
  place `hashbrown 0.15` and `foldhash 0.1` under `gpu-descriptor 0.3.2 ->
  wgpu-hal 29.0.3`, while `hashbrown 0.16` and `foldhash 0.2` enter through
  `gpu-allocator`, `indexmap`, `naga`, `wgpu`, `wgpu-core`, and `wgpu-hal`.
- The renderer-scale follow-up refresh is captured in
  `cargo_search_wgpu_scale_followup.tmp`,
  `cargo_search_gpu_descriptor_scale_followup.tmp`,
  `cargo_info_wgpu_scale_followup.tmp`,
  `cargo_info_gpu_descriptor_scale_followup.tmp`,
  `dep_update_probe_wgpu_stack_scale_followup.tmp`,
  `dependency_budget_scale_followup.tmp`,
  `dependency_budget_wgpu_strict_scale_followup.tmp`,
  `cargo_tree_duplicates_scale_followup.tmp`,
  `dep_inverse_hashbrown_015_scale_followup.tmp`,
  `dep_inverse_hashbrown_016_scale_followup.tmp`,
  `dep_inverse_foldhash_01_scale_followup.tmp`, and
  `dep_inverse_foldhash_02_scale_followup.tmp`. Crates.io still reports
  `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack update
  locks zero packages; normal dependency budget passes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown` in normal workspace and
  all-features scopes.
- The X11 window-selection refresh is captured in
  `cargo_search_wgpu_after_x11_window_selection.tmp`,
  `cargo_search_gpu_descriptor_after_x11_window_selection.tmp`,
  `dep_update_probe_wgpu_stack_after_x11_window_selection.tmp`, and
  `dependency_budget_wgpu_strict_after_x11_window_selection.tmp`. Crates.io
  still reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU
  stack update locks zero packages; strict WGPU-stack budget still fails only
  `foldhash` and `hashbrown` in normal workspace and all-features scopes.
- The PID-owned LeetCodeDaily X11 comparison refresh is captured in
  `cargo_tree_duplicates_latest.tmp` and
  `dependency_budget_wgpu_strict_latest.tmp`. The strict WGPU-stack slice still
  fails only `foldhash` and `hashbrown`; `desktop-platform`,
  `optional-features`, and `unclassified` remain empty. The latest inverse tree
  still places `foldhash 0.1` and `hashbrown 0.15` under
  `gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3`, while `foldhash 0.2` and
  `hashbrown 0.16` enter through `gpu-allocator`, `naga`, `wgpu`,
  `wgpu-core`, `wgpu-hal`, `indexmap`, and proc-macro tooling.
- The LeetCodeDaily first-frame health refresh is captured in
  `cargo_search_wgpu_after_leetcodedaily_initial_frame.tmp`,
  `cargo_search_gpu_descriptor_after_leetcodedaily_initial_frame.tmp`,
  `cargo_info_wgpu_after_leetcodedaily_initial_frame.tmp`,
  `cargo_info_gpu_descriptor_after_leetcodedaily_initial_frame.tmp`,
  `dep_update_probe_wgpu_stack_after_leetcodedaily_initial_frame.tmp`,
  `cargo_tree_duplicates_after_leetcodedaily_initial_frame.tmp`,
  `dep_inverse_hashbrown_015_after_leetcodedaily_initial_frame.tmp`,
  `dep_inverse_hashbrown_016_after_leetcodedaily_initial_frame.tmp`,
  `dep_inverse_foldhash_01_after_leetcodedaily_initial_frame.tmp`,
  `dep_inverse_foldhash_02_after_leetcodedaily_initial_frame.tmp`,
  `dependency_budget_after_leetcodedaily_initial_frame.tmp`, and
  `dependency_budget_wgpu_strict_after_leetcodedaily_initial_frame.tmp`.
  Crates.io still reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the
  dry-run WGPU-stack update locks zero packages; the normal dependency budget
  passes; the strict WGPU-stack slice still fails only `foldhash` and
  `hashbrown` in normal workspace and all-features scopes.
- The HiDPI renderer/robot-helper refresh is captured in
  `dependency_budget_after_nan_sort.tmp` and
  `dependency_budget_wgpu_strict_after_hidpi_nan_sort.tmp`. Normal dependency
  budget passes; strict WGPU-stack budget still fails only `foldhash` and
  `hashbrown` in normal workspace and all-features scopes, while
  `desktop-platform`, `optional-features`, and `unclassified` slices remain
  empty.
- The 2026-05-27 roadmap-continuation refresh is captured in
  `cargo_search_wgpu_continue.tmp`, `cargo_search_gpu_descriptor_continue.tmp`,
  `cargo_info_wgpu_continue.tmp`, `cargo_info_gpu_descriptor_continue.tmp`,
  `dep_update_probe_wgpu_stack_continue.tmp`,
  `dependency_budget_wgpu_strict_continue.tmp`,
  `cargo_tree_duplicates_continue.tmp`,
  `dep_inverse_hashbrown_015_continue.tmp`, and
  `dep_inverse_hashbrown_016_continue.tmp`. Crates.io still reports
  `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack update
  locks zero packages; strict WGPU-stack budget still fails only `foldhash` and
  `hashbrown`; `hashbrown 0.15` remains owned by
  `gpu-descriptor -> wgpu-hal`, while `hashbrown 0.16` remains owned by
  WGPU internals, `gpu-allocator`, `indexmap`, and proc-macro tooling.
- The post-slot-stale-owner refresh is captured in
  `cargo_search_wgpu_after_slot_stale_owner.tmp`,
  `cargo_search_gpu_descriptor_after_slot_stale_owner.tmp`,
  `dep_update_probe_wgpu_stack_after_slot_stale_owner.tmp`,
  `cargo_tree_duplicates_after_slot_stale_owner.tmp`, and
  `dependency_budget_wgpu_strict_after_slot_stale_owner.tmp`. Crates.io still
  reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack
  update locks zero packages; strict WGPU-stack budget still fails only
  `foldhash` and `hashbrown`; `desktop-platform`, `optional-features`, and
  `unclassified` remain empty.
- The post-slot-frame-repair refresh is captured in
  `cargo_search_wgpu_after_slot_frame_repair.tmp`,
  `cargo_search_gpu_descriptor_after_slot_frame_repair.tmp`,
  `dep_update_probe_wgpu_stack_after_slot_frame_repair.tmp`,
  `cargo_tree_duplicates_after_slot_frame_repair.tmp`,
  `dependency_budget_after_slot_frame_repair.tmp`, and
  `dependency_budget_wgpu_strict_after_slot_frame_repair.tmp`. Crates.io still
  reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack
  update locks zero packages; normal dependency budget passes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`;
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-payload-identity refresh is captured in
  `dependency_budget_after_payload_identity.tmp` and
  `dependency_budget_wgpu_strict_after_payload_identity.tmp`. Normal dependency
  budget still passes in workspace and all-features scopes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-composer-retention-root-key refresh is captured in
  `dependency_budget_after_composer_retention_root_key.tmp` and
  `dependency_budget_wgpu_strict_after_composer_retention_root_key.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-state-promote-missing refresh is captured in
  `dependency_budget_after_state_promote_missing.tmp` and
  `dependency_budget_wgpu_strict_after_state_promote_missing.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-state-record-value-result refresh is captured in
  `dependency_budget_after_state_record_value_result.tmp` and
  `dependency_budget_wgpu_strict_after_state_record_value_result.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-state-merge-record-value refresh is captured in
  `dependency_budget_after_state_merge_record_value.tmp` and
  `dependency_budget_wgpu_strict_after_state_merge_record_value.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-payload-length-repair refresh is captured in
  `dependency_budget_after_payload_len_repair.tmp` and
  `dependency_budget_wgpu_strict_after_payload_len_repair.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-node-length-repair refresh is captured in
  `dependency_budget_after_node_len_repair.tmp` and
  `dependency_budget_wgpu_strict_after_node_len_repair.tmp`. Normal dependency
  budget still passes in workspace and all-features scopes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`, while `desktop-platform`,
  `optional-features`, and `unclassified` remain empty.
- The post-detach-segment-repair refresh is captured in
  `dependency_budget_after_detach_segment_repair.tmp` and
  `dependency_budget_wgpu_strict_after_detach_segment_repair.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-corrupt-parent-anchor refresh is captured in
  `dependency_budget_after_corrupt_parent_anchor.tmp` and
  `dependency_budget_wgpu_strict_after_corrupt_parent_anchor.tmp`. Normal
  dependency budget still passes in workspace and all-features scopes; strict
  WGPU-stack budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- The post-stale-value-frame refresh is captured in
  `dependency_budget_after_stale_value_frame.tmp` and
  `dependency_budget_wgpu_strict_after_stale_value_frame.tmp`. Normal dependency
  budget still passes in workspace and all-features scopes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`, while `desktop-platform`,
  `optional-features`, and `unclassified` remain empty.
- The post-keyed-sibling-move refresh is captured in
  `dependency_budget_after_keyed_move.tmp` and
  `dependency_budget_wgpu_strict_after_keyed_move.tmp`. Normal dependency budget
  still passes in workspace and all-features scopes; strict WGPU-stack budget
  still fails only `foldhash` and `hashbrown`, while `desktop-platform`,
  `optional-features`, and `unclassified` remain empty.
- The post-restore-boundary refresh is captured in
  `dependency_budget_after_restore_boundary.tmp` and
  `dependency_budget_wgpu_strict_after_restore_boundary.tmp`. Normal dependency
  budget still passes in workspace and all-features scopes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`, while `desktop-platform`,
  `optional-features`, and `unclassified` remain empty.
- The post-retention-boundary refresh is captured in
  `dependency_budget_after_retention_boundary.tmp` and
  `dependency_budget_wgpu_strict_after_retention_boundary.tmp`. Normal dependency
  budget still passes in workspace and all-features scopes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`, while `desktop-platform`,
  `optional-features`, and `unclassified` remain empty.
- The post-empty-font and visible-runner refresh is captured in
  `cargo_search_wgpu_after_empty_font_docs.tmp`,
  `cargo_search_gpu_descriptor_after_empty_font_docs.tmp`,
  `cargo_info_wgpu_after_empty_font_docs.tmp`,
  `cargo_info_gpu_descriptor_after_empty_font_docs.tmp`,
  `dep_update_probe_wgpu_stack_after_empty_font_docs.tmp`,
  `dependency_budget_after_empty_font_docs.tmp`, and
  `dependency_budget_wgpu_strict_after_empty_font_docs.tmp`. Crates.io still
  reports `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; the dry-run WGPU-stack
  update locks zero packages; normal dependency budget passes; strict WGPU-stack
  budget still fails only `foldhash` and `hashbrown`, while
  `desktop-platform`, `optional-features`, and `unclassified` remain empty.
- `cargo_tree_duplicates_final_probe.tmp` confirms the final visible
  duplicate-version families remain the WGPU-owned `foldhash 0.1/0.2` and
  `hashbrown 0.15/0.16` split. `serde`/`serde_core` appear as repeated roots at
  the same version and remain diagnostic-only.
- `dep_update_probe_wgpu_stack_after_presented_geometry.tmp`: `cargo update -n
  -p wgpu -p gpu-descriptor -p gpu-allocator` locks zero packages. The current
  compatible package set cannot clear the WGPU-stack duplicates through a
  lockfile refresh.
- `cargo_info_indexmap_after_presented_geometry.tmp` and
  `dep_update_indexmap_2140_probe.tmp`: `indexmap 2.14.0` is available, but the
  dry-run adds `hashbrown 0.17.1`, so the repo should stay on `indexmap 2.13.0`
  for the current alignment slice.
- `dep_update_probe_current_after.tmp`: `cargo update -n -p wgpu -p winit -p sctk-adwaita` locks zero packages. The currently compatible package set cannot clear the WGPU-stack duplicates through a lockfile refresh.
- Cache-local WGPU feature probes show that `wgpu 29.0.3` with GLES and
  `indexmap 2.13.0` has no duplicate-version roots, while enabling Vulkan
  reintroduces `gpu-descriptor -> hashbrown 0.15`. The repo-local
  `robot_renderer_micro_no_vulkan.tmp` log proves the no-Vulkan backend matrix
  is not viable for the tested Linux/X11 renderer path.
- `dep_update_indexmap_2130.tmp`: `cargo update -p indexmap --precise 2.13.0` succeeds, removes `hashbrown 0.17.1`, and keeps `indexmap` inside the constraints required by `naga` and `toml_edit`.
- `dep_probe_wgpu25_duplicates.tmp`, `dep_probe_wgpu27_duplicates.tmp`, and `dep_probe_wgpu28_duplicates.tmp` show that adjacent WGPU lines do not clear the strict WGPU slice. WGPU 27/28 retain the `hashbrown 0.15/0.16` split; WGPU 25 removes `foldhash` but still leaves duplicate roots under that older renderer API.
- `dep_update_tiny_skia_012_to_0114.tmp`: `cargo update -p tiny-skia@0.12.0 --precise 0.11.4` succeeds for the default renderer-common path. `cranpose-render-common` now depends on `tiny-skia 0.11.4` with `default-features = false` and only `std`/`simd`, so software text rasterization does not pull PNG decoding.
- `dep_update_resvg_046.tmp` and `dep_update_resvg_047_restore.tmp`: earlier SVG-library probes are superseded by the in-crate parser/rasterizer. Keeping SVG local removes the `resvg/usvg` dependency path instead of tuning a general-purpose SVG stack.
- `services_system_theme.tmp`: `cargo test -p cranpose-services --features system-theme` passes after removing `dark-light`; the service-owned all-features async/getrandom split is gone.
- `svg_tests.tmp`: `cargo test -p cranpose-ui --features svg` passes after replacing `resvg/usvg` with an in-crate SVG parser/rasterizer backed by `tiny-skia 0.11.4`. `dependency_budget_optional_strict_after_wgpu_platform_backend_split.tmp` and `dependency_budget_desktop_strict_after_wgpu_platform_backend_split.tmp` now pass for both normal workspace and all-features scopes and run in CI.
- `wgpu_renderer_linux_features_after_platform_backend_split.tmp`,
  `wgpu_renderer_windows_features_after_platform_backend_split.tmp`,
  `wgpu_renderer_macos_features_after_platform_backend_split.tmp`, and
  `cranpose_facade_linux_wgpu_features_after_platform_backend_split.tmp` prove
  the target-specific backend matrix at Cargo feature resolution level.

## Change Order

The WGPU-stack alignment slice is complete. It removed:

- `foldhash 0.1` vs `0.2`.
- `hashbrown 0.15` vs `0.16`.

Focused gate: `cargo xtask dependency-budget --strict --slice wgpu-stack`.

The local renderer cache ownership slice is complete. Renderer source call
sites now use `cranpose_render_common::bounded_lru_cache::BoundedLruCache`
for:

- WGPU text buffer, size, and prepared-layout caches in `crates/cranpose-render/wgpu/src/lib.rs`.
- WGPU retained image texture and shadow surface caches in `crates/cranpose-render/wgpu/src/render.rs`.
- WGPU retained layer-surface cache in `crates/cranpose-render/wgpu/src/layer_surface_cache.rs`.
- Pixels text metrics cache in `crates/cranpose-render/pixels/src/draw.rs`.

The direct `lru` manifest entries have been removed from
`crates/cranpose-render/wgpu/Cargo.toml` and
`crates/cranpose-render/pixels/Cargo.toml`. The strict WGPU-stack gate now
passes with no duplicate-version roots.

The local software text ownership slice is complete for the pixels renderer.
`cranpose-render-common::software_text_raster` now owns `SoftwareTextFont`,
metrics, layout, cursor/offset mapping, and rasterization. `cargo tree -p
cranpose-render-pixels` shows `ab_glyph` entering through
`cranpose-render-common`, not as a direct pixels dependency. WGPU text rendering
also uses this shared software text backend, so text raster ownership is no
longer split across renderer crates.

The external `pixels` facade dependency cleanup is complete. The in-tree
software renderer does not use the `pixels` crate, so `cranpose/renderer-pixels`
no longer enables `dep:pixels` and all-features builds no longer receive
`pixels`' default WGPU/Vulkan feature set. `cargo xtask dependency-budget`
checks `cargo tree -p cranpose --no-default-features --features renderer-pixels`
and rejects `pixels`, `wgpu`, `wgpu-core`, `wgpu-hal`, and `naga` in that
feature graph.

The `indexmap` lockfile slice is complete. `indexmap` is pinned to `2.13.0`,
which satisfies the current `naga` and `toml_edit` constraints while using
`hashbrown 0.16.1`; this removes the separate `hashbrown 0.17.1` root. The
strict WGPU-stack gate still fails because WGPU 29 owns both `hashbrown 0.15`
through `gpu-descriptor` and `hashbrown 0.16` through `gpu-allocator` and WGPU
compiler/runtime internals.

The default desktop platform slice is complete. `cranpose-render-common` now
uses the same `tiny-skia 0.11.4` line as `sctk-adwaita -> winit-wayland ->
winit`, with PNG decoding disabled for software text rasterization.

Focused gate: `cargo xtask dependency-budget --strict --slice desktop-platform`.

That focused gate now passes for normal workspace and all-features builds.
Optional SVG uses the same `tiny-skia 0.11.4` line as the desktop platform
stack and no longer owns `tiny-skia 0.12` or `tiny-skia-path 0.12`.

The optional all-features cleanup slice is complete.

The service-owned portion of this slice is complete. `cranpose-services` no
longer depends on `dark-light`, so `async-channel`, `event-listener`, and
`getrandom` no longer appear in the all-features duplicate-version slice.
The SVG-owned portion is complete. `cranpose-ui/svg` no longer depends on
`resvg/usvg`, so `roxmltree`, `tiny-skia 0.12`, and `tiny-skia-path 0.12` no
longer appear in the all-features duplicate-version slice.

Focused gate: `cargo xtask dependency-budget --strict --slice optional-features`.

The post-active-group-handle refresh confirms the dependency blocker is
unchanged. `cargo_search_wgpu_after_active_group_handle.tmp` reports
`wgpu 29.0.3`, `cargo_search_gpu_descriptor_after_active_group_handle.tmp`
reports `gpu-descriptor 0.3.2`, and
`dep_update_probe_wgpu_stack_after_active_group_handle.tmp` locks zero packages
in dry-run mode. `dependency_budget_after_active_group_handle.tmp` passes the
normal tracked budget, while
`dependency_budget_wgpu_strict_after_active_group_handle.tmp` still fails only
the WGPU-stack `foldhash` and `hashbrown` families.

The 2026-05-28 dependency refresh confirms the dependency blocker is still
unchanged. `cargo_search_wgpu_2026_05_28.tmp` and
`cargo_info_wgpu_2026_05_28.tmp` report `wgpu 29.0.3`;
`cargo_search_gpu_descriptor_2026_05_28.tmp` and
`cargo_info_gpu_descriptor_2026_05_28.tmp` report `gpu-descriptor 0.3.2`;
`dep_update_probe_wgpu_stack_2026_05_28.tmp` locks zero packages in dry-run
mode. `dependency_budget_2026_05_28.tmp` passes the tracked budget, while
`dependency_budget_wgpu_strict_2026_05_28.tmp` still fails only the
WGPU-stack-owned `foldhash` and `hashbrown` families. The inverse probes
`dep_inverse_hashbrown_015_2026_05_28.tmp` and
`dep_inverse_foldhash_01_2026_05_28.tmp` still route the older roots through
`gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3`.

The post-text-focus refresh remains unchanged. `dependency_budget_text_focus_stale.tmp`
passes the tracked duplicate budget, while
`dependency_budget_wgpu_strict_text_focus_stale.tmp` still fails only the
WGPU-stack-owned `foldhash` and `hashbrown` families in normal and all-features
workspace scopes.

The post-scroll-invalidation refresh remains unchanged. `dependency_budget_after_scroll_invalidation.tmp`
passes the tracked duplicate budget; `dependency_budget_desktop_strict_after_scroll_invalidation.tmp`
and `dependency_budget_optional_strict_after_scroll_invalidation.tmp` pass the
strict clean-slice gates; `dependency_budget_wgpu_strict_after_scroll_invalidation.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes.

The post-next-frame-wake refresh remains unchanged. `dependency_budget_after_next_frame_wake.tmp`
passes the tracked duplicate budget; `dependency_budget_desktop_strict_after_next_frame_wake.tmp`
and `dependency_budget_optional_strict_after_next_frame_wake.tmp` pass the
strict clean-slice gates; `dependency_budget_wgpu_strict_after_next_frame_wake.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes.

The post-detach-subtree-span refresh remains unchanged. `dependency_budget_after_detach_subtree_len.tmp`
passes the tracked duplicate budget; `dependency_budget_wgpu_strict_after_detach_subtree_len.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes, while `desktop-platform`,
`optional-features`, and `unclassified` remain empty.

The post-empty-detached-scope refresh remains unchanged. `dependency_budget_after_empty_detached_scope.tmp`
passes the tracked duplicate budget; `dependency_budget_wgpu_strict_after_empty_detached_scope.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes, while `desktop-platform`,
`optional-features`, and `unclassified` remain empty.

The post-stale-range refresh remains unchanged. `dependency_budget_after_stale_range.tmp`
passes the tracked duplicate budget; `dependency_budget_wgpu_strict_after_stale_range.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes. `dep_update_probe_wgpu_stack_after_stale_range.tmp`
locks zero packages in dry-run mode. The inverse probes
`cargo_tree_i_hashbrown_015_stale_range.tmp`,
`cargo_tree_i_foldhash_015_stale_range.tmp`,
`cargo_tree_i_hashbrown_016_stale_range.tmp`, and
`cargo_tree_i_foldhash_020_stale_range.tmp` confirm the older roots still enter
through `gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3`, while the newer roots enter
through `gpu-allocator 0.28.0`, `naga 29.0.3`, `wgpu 29.0.3`,
`wgpu-core 29.0.3`, and `wgpu-hal 29.0.3`.

The post-subcompose-policy-boundary refresh remains unchanged.
`dependency_budget_after_subcompose_policy_boundary.tmp` passes the tracked
duplicate budget; `dependency_budget_wgpu_strict_after_subcompose_policy_boundary.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes.

The post-snapshot-apply-readable refresh remains unchanged.
`dependency_budget_after_snapshot_apply_readable.tmp` passes the tracked
duplicate budget; `dependency_budget_wgpu_strict_after_snapshot_apply_readable.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes.

The post-FPS-gate refresh remains unchanged.
`dependency_budget_after_perf_fps_gate.tmp` passes the tracked duplicate budget;
`dependency_budget_wgpu_strict_after_perf_fps_gate.tmp` still fails only the
WGPU-stack-owned `foldhash` and `hashbrown` families in normal and all-features
workspace scopes. `cargo_tree_duplicates_after_perf_fps_gate.tmp`,
`dep_inverse_hashbrown_015_after_perf_fps_gate.tmp`,
`dep_inverse_hashbrown_016_after_perf_fps_gate.tmp`,
`dep_inverse_foldhash_015_after_perf_fps_gate.tmp`, and
`dep_inverse_foldhash_020_after_perf_fps_gate.tmp` confirm the older
`hashbrown 0.15` / `foldhash 0.1` roots still enter through
`gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3`, while the newer
`hashbrown 0.16` / `foldhash 0.2` roots enter through WGPU 29 internals,
`gpu-allocator`, `naga`, `wgpu`, `wgpu-core`, `wgpu-hal`, `indexmap`, and
proc-macro tooling.

The post-web-release-profile refresh remains unchanged.
`cargo_search_wgpu_after_wasm_release_profile.tmp`,
`cargo_search_gpu_descriptor_after_wasm_release_profile.tmp`,
`cargo_info_wgpu_after_wasm_release_profile.tmp`, and
`cargo_info_gpu_descriptor_after_wasm_release_profile.tmp` still report
`wgpu 29.0.3` and `gpu-descriptor 0.3.2` as the current released crates.
`dep_update_probe_wgpu_stack_after_wasm_release_profile.tmp` locks zero
packages in dry-run mode. `dependency_budget_after_wasm_release_profile.tmp`
passes the tracked duplicate budget, while
`dependency_budget_wgpu_strict_after_wasm_release_profile.tmp` still fails only
the WGPU-stack-owned `foldhash` and `hashbrown` families in normal and
all-features workspace scopes. `cargo_tree_duplicates_workspace_after_wasm_release_profile.tmp`,
`cargo_tree_duplicates_after_wasm_release_profile.tmp`,
`dep_inverse_hashbrown_015_after_wasm_release_profile.tmp`,
`dep_inverse_hashbrown_016_after_wasm_release_profile.tmp`,
`dep_inverse_foldhash_015_after_wasm_release_profile.tmp`, and
`dep_inverse_foldhash_020_after_wasm_release_profile.tmp` keep the older
`hashbrown 0.15` / `foldhash 0.1` roots under
`gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3`, while the newer
`hashbrown 0.16` / `foldhash 0.2` roots enter through WGPU 29 internals,
`gpu-allocator 0.28.0`, `naga 29.0.3`, `wgpu 29.0.3`,
`wgpu-core 29.0.3`, `wgpu-hal 29.0.3`, `indexmap 2.13.0`, and
proc-macro tooling. No library versions changed in this refresh.

The post-WASM-size-gate refresh remains unchanged.
`dependency_budget_after_wasm_size_gate.tmp` passes the tracked duplicate
budget. `dependency_budget_wgpu_strict_after_wasm_size_gate.tmp` still fails
only the WGPU-stack-owned `foldhash` and `hashbrown` families in normal and
all-features workspace scopes, with `desktop-platform`, `optional-features`,
and `unclassified` empty. The web size gate is a local build-script budget and
does not change the dependency graph.

The post-initial-native-registry refresh remains unchanged.
`cargo_search_wgpu_after_initial_registry.tmp`,
`cargo_search_gpu_descriptor_after_initial_registry.tmp`,
`cargo_info_wgpu_after_initial_registry.tmp`, and
`cargo_info_gpu_descriptor_after_initial_registry.tmp` still report
`wgpu 29.0.3` and `gpu-descriptor 0.3.2` as the current released crates.
`dep_update_probe_wgpu_stack_after_initial_registry.tmp` locks zero packages in
dry-run mode. `dependency_budget_after_initial_registry.tmp` passes the tracked
duplicate budget; `dependency_budget_wgpu_strict_after_initial_registry.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes. `cargo_tree_duplicates_after_initial_registry.tmp`
keeps the older roots under `gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3` and the
newer roots under WGPU 29 internals, `gpu-allocator`, `naga`, `indexmap`, and
proc-macro tooling.

The 2026-05-31 registry and dependency-budget refresh remains unchanged.
`cargo_search_wgpu_2026_05_31.tmp` and
`cargo_search_gpu_descriptor_2026_05_31.tmp` still report `wgpu 29.0.3` and
`gpu-descriptor 0.3.2` as the current released crates. `dependency_budget_2026_05_31.tmp`
passes the tracked duplicate budget; `dependency_budget_wgpu_strict_2026_05_31.tmp`
still fails only the WGPU-stack-owned `foldhash` and `hashbrown` families in
normal and all-features workspace scopes, with `desktop-platform`,
`optional-features`, and `unclassified` empty. `cargo_tree_duplicates_2026_05_31.tmp`
keeps the older roots under `gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3` and the
newer roots under WGPU 29 internals, `gpu-allocator`, `naga`, `indexmap`, and
proc-macro tooling. No library versions changed in this refresh.

The WGPU-stack alignment refresh patches `gpu-descriptor 0.3.2` to upstream
commit `79804e422186805f1ff5ab3d8310c07c145a6731`, the commit that aligns its
`hashbrown` dependency to `0.16`. `cargo update -p gpu-descriptor` removed
`foldhash 0.1.5`, `hashbrown 0.15.5`, the crates.io `gpu-descriptor 0.3.2`,
and the crates.io `gpu-descriptor-types 0.2.0`, then added the pinned upstream
`gpu-descriptor` and `gpu-descriptor-types` packages. `dependency_budget_zero_allowlist.tmp`,
`dependency_budget_strict_zero_allowlist.tmp`, and
`dependency_budget_wgpu_strict_zero_allowlist.tmp` pass with no duplicate roots
in normal and all-features workspace scopes. `xtask_dependency_budget_zero_allowlist.tmp`
passes after removing the old `foldhash`/`hashbrown` allowlist from the budget
tool, so the default CI gate now rejects any duplicate-version family.

The desktop native-HTTP gate refresh keeps the service-owned HTTP stack out of
the default desktop binary. `desktop_app_default_tree_after_concurrency_split.tmp`
contains no `reqwest`, `rustls`, `hyper`, `aws-lc`, or `webpki` entries for the
default `desktop-app` graph, while `desktop_app_desktop_http_check.tmp` proves
the explicit `desktop-http` app feature still type-checks. Native ordered
concurrency remains default-native through the small `pollster` executor, so
mock/custom HTTP clients keep the same parallel batch behavior without enabling
`reqwest`. The production size gate
`binary_size_release_small_after_concurrency_split.tmp` passes at 28,428,784
bytes (27.11 MiB), after the previous default graph failed at 31,968,392 bytes
because it pulled `cranpose-services/http-native`.
`no_default_build_after_concurrency_split.tmp`,
`all_features_check_after_concurrency_split.tmp`, and
`dependency_budget_after_concurrency_split.tmp` pass after the feature split;
that earlier refresh still had the tracked WGPU-stack `foldhash` and
`hashbrown` families, which are superseded by the later
`gpu-descriptor` hashbrown-0.16 alignment refresh.
`dependency_budget_wgpu_strict_after_concurrency_split.tmp` documents the
pre-alignment strict WGPU-stack failure, and
`cargo_tree_duplicates_after_concurrency_split.tmp` keeps the older roots under
`gpu-descriptor 0.3.2 -> wgpu-hal 29.0.3` while newer roots enter through WGPU
29 internals, `gpu-allocator`, `naga`, `indexmap`, and proc-macro tooling.

## Validation Gate

After an approved dependency alignment change:

- `cargo tree --duplicates --workspace`.
- `cargo tree --duplicates --workspace --all-features`.
- `cargo xtask dependency-budget`.
- `cargo xtask dependency-budget --explain`.
- `cargo xtask dependency-budget --strict`.
- `cargo build --workspace --no-default-features`.
- `cargo check -p cranpose --no-default-features`.
- `cargo check -p cranpose-app-shell --no-default-features`.
- `cargo check -p cranpose --no-default-features --features desktop,renderer-wgpu`.
- `cargo check -p cranpose-testing --no-default-features`.
- `cargo check -p cranpose-testing --no-default-features --features desktop-robot`.
- `cargo test -p cranpose-testing --no-default-features`.
- `cargo test > 1.tmp 2>&1`.
- `cargo clippy > 2.tmp 2>&1`.
- `cargo fmt`.
- `apps/desktop-demo/build-web.sh`.
- `apps/android-demo/android ./gradlew :app:assembleRelease`.
- `./run_robot_test.sh --sequential`.

Large diagnostic files must stay out of tmpfs-backed directories. Use `CRANPOSE_ROBOT_OUTPUT_DIR`, a non-tmpfs `TMPDIR`, or repo-local small logs.
