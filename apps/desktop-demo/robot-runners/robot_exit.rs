#![allow(dead_code)]

use std::{
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use cranpose::Robot;

const SHUTDOWN_WATCHDOG: Duration = Duration::from_secs(15);

pub fn fail(robot: &Robot, message: &str) -> ! {
    report(message);
    let _ = robot.exit();
    std::process::exit(1);
}

pub fn fail_without_shutdown(message: &str) -> ! {
    report(message);
    std::process::exit(1);
}

pub fn fail_and_await_shutdown(robot: &Robot, failed: &'static AtomicBool, message: &str) -> ! {
    report(message);
    failed.store(true, Ordering::Relaxed);
    thread::spawn(|| {
        thread::sleep(SHUTDOWN_WATCHDOG);
        std::process::exit(1);
    });
    let _ = robot.exit();
    loop {
        thread::sleep(SHUTDOWN_WATCHDOG);
    }
}

fn report(message: &str) {
    println!("FATAL: {message}");
}

pub fn arm_timeout(seconds: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(seconds));
        println!("FATAL: test timed out after {seconds} seconds");
        std::process::exit(1);
    });
}

pub fn exit_code(failed: &AtomicBool) -> ExitCode {
    if failed.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
