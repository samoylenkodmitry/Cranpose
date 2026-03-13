# Time Wasters

- Parallel execution of robot examples is flaky on this machine even without Cranelift. `16`, `8`, and `4` parallel workers produced intermittent segfaults or timeouts, while sequential execution passed `80/80`, so `run_robot_test.sh` now defaults to sequential mode and leaves `--parallel N` as an explicit opt-in.
- Perf harness trap: do not run multiple GPU perf scenarios in parallel on this machine. The numbers become meaningless because the scenarios contend for the same GPU/driver state. Perf comparisons for `robot_perf_harness` must be sequential and single-scenario.
