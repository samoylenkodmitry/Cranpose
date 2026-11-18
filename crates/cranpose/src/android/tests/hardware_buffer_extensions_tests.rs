use super::*;

fn all() -> Vec<&'static CStr> {
    EXTENSIONS.to_vec()
}

#[test]
fn a_driver_with_every_extension_adds_the_ones_the_device_lacks() {
    let enabled = [
        c"VK_KHR_swapchain",
        c"VK_KHR_dedicated_allocation",
        c"VK_KHR_bind_memory2",
    ];
    let mut available = all();
    available.push(c"VK_KHR_swapchain");
    let added = extensions_to_add(&available, &enabled).expect("every extension is there");
    assert_eq!(
        added,
        [
            c"VK_ANDROID_external_memory_android_hardware_buffer",
            c"VK_EXT_queue_family_foreign",
            c"VK_KHR_sampler_ycbcr_conversion",
            c"VK_KHR_external_memory",
            c"VK_KHR_get_memory_requirements2",
        ]
    );
}

#[test]
fn a_device_that_has_them_all_adds_nothing() {
    let all = all();
    assert_eq!(extensions_to_add(&all, &all), Some(Vec::new()));
}

#[test]
fn a_driver_that_lacks_one_extension_imports_nothing() {
    for (lacking, name) in EXTENSIONS.iter().enumerate() {
        let mut available = all();
        available.remove(lacking);
        assert_eq!(extensions_to_add(&available, &[]), None, "without {name:?}");
    }
}

#[test]
fn a_device_imports_only_with_every_extension_enabled() {
    let all = all();
    assert!(imports(&all));
    for (lacking, name) in all.iter().enumerate() {
        let mut enabled = all.clone();
        enabled.remove(lacking);
        assert!(!imports(&enabled), "without {name:?}");
    }
}
