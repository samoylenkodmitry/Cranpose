use super::*;

#[cfg(unix)]
#[test]
fn a_folder_that_does_not_exist_yet_is_measured_by_the_one_above_it() {
    let _services = crate::registry::test_service_guard();
    clear_platform_storage_space();
    let scratch = crate::test_scratch_dir("storage");
    let here = available_storage(&scratch).expect("the scratch folder's volume");
    let later = available_storage(&scratch.join("models").join("speech.gguf"))
        .expect("the volume of a folder made later");

    assert!(here > 0);
    // Other processes write meanwhile; the same volume differs by little.
    assert!(here.abs_diff(later) < 64 * 1024 * 1024);
}

#[test]
fn a_platform_measure_replaces_the_file_system_one() {
    struct Fixed;
    impl StorageSpace for Fixed {
        fn available(&self, _path: &Path) -> Option<u64> {
            Some(42)
        }
    }
    let _services = crate::registry::test_service_guard();
    set_platform_storage_space(Arc::new(Fixed));
    let scratch = crate::test_scratch_dir("storage-platform");

    assert_eq!(available_storage(&scratch), Some(42));
    clear_platform_storage_space();
}
