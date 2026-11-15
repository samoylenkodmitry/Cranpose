/// Restricts the calling thread to the fast-capacity CPUs, when the
/// machine has distinguishable ones. `role` names the thread in the log
/// line so an A/B trace shows who pinned where.
pub fn pin_current_thread_to_fast_cores(role: &str) {
    imp::pin(role);
}

// Lets the calling thread, which no frame waits on, run on every CPU below
// the priority of the threads that make frames. A thread starts on its
// creator's CPUs at its creator's priority, so a thread the producer made
// after it pinned itself would take the fast cores from the frame threads.
// Threads the calling thread makes, as a GPU driver's compiler does, start
// where it runs now.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_current_thread_in_background(role: &str) {
    imp::run_in_background(role);
}

/// The calling thread's id in the OS scheduler, where scheduler hints
/// name threads by one.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn current_thread_id() -> Option<i32> {
    imp::thread_id()
}

#[cfg(any(target_os = "android", target_os = "linux"))]
mod imp {
    use std::path::Path;

    pub(super) fn pin(role: &str) {
        if cranpose_core::DIAGNOSTICS.core_pin.parse::<u8>() == Some(0) {
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

    // The nice value of Android's `THREAD_PRIORITY_BACKGROUND`: a thread at
    // 0 takes about nine tenths of a CPU it shares with one at this value.
    const BACKGROUND_NICE: i32 = 10;

    pub(super) fn run_in_background(role: &str) {
        let capacities = read_cpu_capacities(Path::new("/sys/devices/system/cpu"));
        // Only a machine with fast cores pins threads to them.
        if fast_cpus(&capacities).is_some()
            && capacities
                .iter()
                .all(|&(cpu, _)| cpu < rustix::thread::CpuSet::MAX_CPU)
        {
            let mut set = rustix::thread::CpuSet::new();
            for &(cpu, _) in &capacities {
                set.set(cpu);
            }
            if let Err(error) = rustix::thread::sched_setaffinity(None, &set) {
                log::warn!("[core-pin] {role}: sched_setaffinity failed: {error}");
            }
        }
        // On Linux a thread's priority is its own: `None` names the caller.
        match rustix::process::setpriority_process(None, BACKGROUND_NICE) {
            Ok(()) => log::info!("[core-pin] {role}: every cpu, nice {BACKGROUND_NICE}"),
            Err(error) => log::warn!("[core-pin] {role}: setpriority failed: {error}"),
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn run_in_background(_role: &str) {}

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn thread_id() -> Option<i32> {
        None
    }
}

#[cfg(all(test, any(target_os = "android", target_os = "linux")))]
#[path = "tests/fast_cores_tests.rs"]
mod tests;
