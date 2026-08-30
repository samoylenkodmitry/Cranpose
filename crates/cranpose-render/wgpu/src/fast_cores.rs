/// Restricts the calling thread to the fast-capacity CPUs, when the
/// machine has distinguishable ones. `role` names the thread in the log
/// line so an A/B trace shows who pinned where.
pub fn pin_current_thread_to_fast_cores(role: &str) {
    imp::pin(role);
}

#[cfg(any(target_os = "android", target_os = "linux"))]
mod imp {
    use std::path::Path;

    pub(super) fn pin(role: &str) {
        if std::env::var("CRANPOSE_CORE_PIN").is_ok_and(|value| value.trim() == "0") {
            log::info!("[core-pin] {role}: disabled by CRANPOSE_CORE_PIN=0");
            return;
        }
        let capacities = read_cpu_capacities(Path::new("/sys/devices/system/cpu"));
        let Some(fast) = fast_cpus(&capacities) else {
            log::debug!("[core-pin] {role}: symmetric or unpublished topology, not pinning");
            return;
        };
        if fast
            .iter()
            .any(|&cpu| cpu >= rustix::thread::CpuSet::MAX_CPU)
        {
            log::debug!("[core-pin] {role}: cpu index beyond CpuSet capacity, not pinning");
            return;
        }
        let mut set = rustix::thread::CpuSet::new();
        for &cpu in &fast {
            set.set(cpu);
        }
        match rustix::thread::sched_setaffinity(None, &set) {
            Ok(()) => log::info!("[core-pin] {role}: eligible cpus {fast:?}"),
            Err(error) => log::warn!("[core-pin] {role}: sched_setaffinity failed: {error}"),
        }
    }

    pub(super) fn read_cpu_capacities(base: &Path) -> Vec<(usize, u64)> {
        let mut capacities = Vec::new();
        let Ok(entries) = std::fs::read_dir(base) else {
            return capacities;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(index) = name
                .to_str()
                .and_then(|name| name.strip_prefix("cpu"))
                .and_then(|digits| digits.parse::<usize>().ok())
            else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(entry.path().join("cpu_capacity")) else {
                continue;
            };
            let Ok(capacity) = text.trim().parse::<u64>() else {
                continue;
            };
            capacities.push((index, capacity));
        }
        capacities.sort_unstable();
        capacities
    }

    pub(super) fn fast_cpus(capacities: &[(usize, u64)]) -> Option<Vec<usize>> {
        let min = capacities.iter().map(|&(_, capacity)| capacity).min()?;
        let max = capacities.iter().map(|&(_, capacity)| capacity).max()?;
        if min == max {
            return None;
        }
        let fast: Vec<usize> = capacities
            .iter()
            .filter(|&&(_, capacity)| capacity > min)
            .map(|&(index, _)| index)
            .collect();
        (fast.len() >= 2).then_some(fast)
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
mod imp {
    pub(super) fn pin(_role: &str) {}
}

#[cfg(all(test, any(target_os = "android", target_os = "linux")))]
mod tests {
    use super::imp::{fast_cpus, read_cpu_capacities};

    fn scratch_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/test-output/fast-cores")
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
}
