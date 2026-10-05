fn main() {
    if let Err(error) = slint_build::compile("ui/gauntlet.slint") {
        panic!("ui/gauntlet.slint: {error}");
    }
}
