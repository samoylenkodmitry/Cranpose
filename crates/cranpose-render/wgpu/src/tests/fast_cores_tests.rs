use super::imp::{fast_cpus, read_cpu_capacities};

fn scratch_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/test-output/fast-cores")
}

#[test]
fn a_three_tier_topology_keeps_every_cluster_above_the_weakest() {
    let kirin_980 = [
        (0, 317),
        (1, 317),
        (2, 317),
        (3, 317),
        (4, 752),
        (5, 752),
        (6, 1024),
        (7, 1024),
    ];
    assert_eq!(fast_cpus(&kirin_980), Some(vec![4, 5, 6, 7]));
}

#[test]
fn a_symmetric_topology_pins_nothing() {
    assert_eq!(
        fast_cpus(&[(0, 1024), (1, 1024), (2, 1024), (3, 1024)]),
        None
    );
    assert_eq!(fast_cpus(&[]), None);
}

#[test]
fn a_single_fast_core_is_not_worth_serializing_two_threads_on() {
    assert_eq!(fast_cpus(&[(0, 317), (1, 317), (2, 317), (3, 1024)]), None);
}

#[test]
fn capacities_come_from_the_cpu_directories_and_ignore_everything_else() {
    let base = scratch_root().join(format!(
        "cpus-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    for (name, capacity) in [("cpu0", "317\n"), ("cpu1", "1024\n")] {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cpu_capacity"), capacity).unwrap();
    }
    std::fs::create_dir_all(base.join("cpufreq")).unwrap();
    std::fs::create_dir_all(base.join("cpu2")).unwrap();

    assert_eq!(read_cpu_capacities(&base), vec![(0, 317), (1, 1024)]);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_missing_sysfs_base_reads_as_no_capacities() {
    let ghost = scratch_root().join("nowhere");
    assert_eq!(read_cpu_capacities(&ghost), Vec::new());
}
