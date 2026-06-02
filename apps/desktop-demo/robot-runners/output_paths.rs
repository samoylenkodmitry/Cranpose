use std::ffi::OsStr;
use std::path::PathBuf;

pub fn diagnostic_path(name: &str) -> PathBuf {
    let root = temp_root();
    std::fs::create_dir_all(&root)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
    root.join(name)
}

fn temp_root() -> PathBuf {
    if let Some(path) = std::env::var_os("CRANPOSE_ROBOT_OUTPUT_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("TMPDIR") {
        let path = PathBuf::from(path);
        let tmpfs_root = ["/", "tmp"].concat();
        if path.as_os_str() != OsStr::new(&tmpfs_root) {
            return path;
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".cranpose-tmp")
}
