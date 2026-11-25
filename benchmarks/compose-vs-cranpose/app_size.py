"""Logical bytes in the app artifact supplied to the benchmark."""

def artifact_bytes(path):
    if path.is_file():
        return path.stat().st_size
    if not path.is_dir():
        raise FileNotFoundError(path)
    # A macOS framework bundle links several names to its versioned library.
    return sum(file.stat().st_size for file in path.rglob('*')
               if not file.is_symlink() and file.is_file())
