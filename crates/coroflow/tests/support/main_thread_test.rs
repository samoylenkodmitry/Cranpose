// Runs the one test of a `harness = false` binary on its main thread. The
// counting allocator counts every thread. libtest runs a test on a thread of
// its own, and its main thread allocates while that test's first region is
// open. A binary without libtest has only the threads its test starts.
//
// `main_thread_test!(name)` takes the libtest arguments passed by cargo test
// and nextest: `--list` names the test, and `--ignored` and `--bench` run
// nothing. The binary ignores name filters: nextest passes the name of its
// one test.

macro_rules! main_thread_test {
    ($test:ident) => {
        fn main() {
            main_thread_test::run(stringify!($test), $test);
        }
    };
}

#[expect(
    clippy::print_stdout,
    reason = "cargo test and nextest read libtest's list and result lines"
)]
pub(crate) fn run(name: &str, test: fn()) {
    let mut list = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--ignored" | "--bench" => return,
            "--list" => list = true,
            _ => {}
        }
    }
    if list {
        println!("{name}: test");
        return;
    }
    test();
    println!("test {name} ... ok");
}
