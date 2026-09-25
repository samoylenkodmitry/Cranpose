/// Restricts the calling thread to the fast-capacity CPUs, when the
/// machine has distinguishable ones. `role` names the thread in the log
/// line so an A/B trace shows who pinned where.
pub fn pin_current_thread_to_fast_cores(role: &str) {
    imp::pin(role);
}

/// The calling thread's id in the OS scheduler, where scheduler hints
/// name threads by one.
pub(crate) fn current_thread_id() -> Option<i32> {
    imp::thread_id()
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

    pub(super) fn thread_id() -> Option<i32> {
        Some(rustix::thread::gettid().as_raw_nonzero().get())
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

    pub(super) fn thread_id() -> Option<i32> {
        None
    }
}

#[cfg(all(test, any(target_os = "android", target_os = "linux")))]
#[path = "tests/fast_cores_tests.rs"]
mod tests;
