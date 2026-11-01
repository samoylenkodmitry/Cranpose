#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DuplicateDebt {
    pub(crate) family: &'static str,
    pub(crate) reason: &'static str,
}

pub(crate) const WORKSPACE_DUPLICATE_DEBT: &[DuplicateDebt] = &[
    DuplicateDebt {
        family: "bitflags",
        reason: "Wry 0.57 requires webkit2gtk 2 and javascriptcore-rs 1, which use bitflags 1 while the workspace uses 2",
    },
    DuplicateDebt {
        family: "heck",
        reason: "Wry's GTK 3 stack uses glib-macros 0.18 with heck 0.4 while the workspace uses 0.5",
    },
    DuplicateDebt {
        family: "proc-macro-crate",
        reason: "Wry's GTK 3 stack requires gtk3-macros 0.18 and glib-macros 0.18, which use proc-macro-crate 1 and 2 while the workspace uses 3",
    },
    DuplicateDebt {
        family: "serde_spanned",
        reason: "Wry's GTK 3 system-deps 6 requires toml 0.8 and serde_spanned 0.6 while the workspace uses toml 1 and serde_spanned 1",
    },
    DuplicateDebt {
        family: "toml",
        reason: "Wry's GTK 3 sys crates require system-deps 6 with toml 0.8 while the workspace uses toml 1",
    },
    DuplicateDebt {
        family: "toml_datetime",
        reason: "Follows the GTK 3 toml 0.8 and proc-macro-crate 1/2 split",
    },
    DuplicateDebt {
        family: "toml_edit",
        reason: "GTK 3's proc-macro-crate 1/2 and toml 0.8 require toml_edit 0.19/0.20 while proc-macro-crate 3 uses 0.25",
    },
    DuplicateDebt {
        family: "winnow",
        reason: "Follows the GTK 3 toml_edit 0.19/0.20 split via winnow 0.5",
    },
    DuplicateDebt {
        family: "hashbrown",
        reason: "gpu-allocator 0.28.0 (latest, and on upstream main) holds ^0.16 for wgpu-hal 30 while wgpu, AccessKit and indexmap are on ^0.17",
    },
    DuplicateDebt {
        family: "jni-sys",
        reason: "ndk 0.9.0 and ndk-sys 0.6.0 (latest) pin jni-sys ^0.3 while jni 0.22 is on ^0.4",
    },
    DuplicateDebt {
        family: "miniz_oxide",
        reason: "flate2 1.1.10 moved to ^0.9 while png 0.18.1 (latest) is still on ^0.8",
    },
    DuplicateDebt {
        family: "objc2",
        reason: "accesskit_macos 0.27 holds objc2 0.5 while winit-appkit 0.31.0-beta.3 is on 0.6 (AccessKit/accesskit#616)",
    },
    DuplicateDebt {
        family: "objc2-app-kit",
        reason: "follows the objc2 split via accesskit_macos 0.27",
    },
    DuplicateDebt {
        family: "objc2-foundation",
        reason: "follows the objc2 split via accesskit_macos 0.27",
    },
    DuplicateDebt {
        family: "syn",
        reason: "serde_derive, thiserror-impl 2, bytemuck_derive, wasm-bindgen-macro and async-recursion use syn 3 while jni-macros, num_enum_derive and zerocopy-derive use syn 2",
    },
    DuplicateDebt {
        family: "thiserror",
        reason: "ndk 0.9.0 (latest) pins thiserror ^1 while the workspace is on 2",
    },
    DuplicateDebt {
        family: "thiserror-impl",
        reason: "follows the thiserror split via ndk 0.9.0",
    },
    DuplicateDebt {
        family: "rustc-hash",
        reason: "fluent-bundle 0.16.0 and type-map 0.5.1 require rustc-hash 2.x (Askama 0.16 in UniFFI's all-features graph also requires 2.x), while wgpu-core 30.0.7, naga 30.0.1, and naga-types 30.0.1 require 1.1.x; unifying these incompatible major-version constraints would require changing upstream dependencies",
    },
];

pub(crate) const ALL_FEATURES_EXTRA_DUPLICATE_DEBT: &[DuplicateDebt] = &[DuplicateDebt {
    family: "env_filter",
    reason: "android_logger 0.15.1 (latest) pins env_filter ^0.1 while env_logger 0.11 is past 1.0",
}];
