#!/usr/bin/env python3
"""Start new worktrees with a warm cargo target cloned from a seed worktree.

A fresh worktree otherwise builds every crate of the workspace from nothing:
minutes of CPU for each one, and a full target directory on disk for each one.
A seed is a locked worktree at origin/main whose target was built with the
commands agents run. `clone` copies the seed's target into a new worktree -- a
copy-on-write clone on APFS, a reflink where the filesystem has them, a plain
copy elsewhere -- and then makes cargo's own freshness check right there:

* Cargo calls a unit fresh when no input file is newer than its last build.
  Git writes every file of a new worktree with the current time, so the clone
  would rebuild everything. A file or directory whose committed content equals
  the seed's, and which neither tree has edited, gets the seed's mtime; every
  other one keeps its new mtime and its units rebuild.
* Cargo stores local sources relative to the package root and rewrites a build
  script's own OUT_DIR when the target moves. Records that name other seed
  paths are pointed at the new tree when they only list files a unit read, and
  invalidated otherwise: a build script that watches a seed file by absolute
  path, or a unit whose compiled code holds the seed's path, rebuilds here.

`refresh` makes the next seed: a new worktree at origin/main cloned from the
current seed, warmed, locked, and then named in `git config cranpose.seed`.
The previous seed is removed once no clone reads it. The repository's
post-checkout hook runs `clone` for every new worktree and starts a refresh in
the background when origin/main has moved past the seed.
"""
import argparse
import fcntl
import os
import shutil
import subprocess
import sys
import time
from contextlib import contextmanager
from pathlib import Path

SEED_CONFIG = "cranpose.seed"
SEED_LOCK_REASON = "cargo seed"
# What agents build, so the seed holds the units their first build asks for.
# The lint commands are the justfile's, flags included: clippy keys its results
# on its arguments, and a different flag would build a different unit.
WARM_COMMANDS = [
    ["python3", "scripts/ci/clippy_budget.py", "--count"],
    ["cargo", "clippy", "-p", "cranpose", "--all-targets", "--", "-D", "warnings"],
    ["cargo", "build", "--workspace"],
    ["cargo", "test", "--workspace", "--no-run"],
    ["just", "clippy-robot"],
    ["just", "clippy-wasm"],
    ["just", "clippy-android"],
]


# Git exports these to hooks; left set, `git -C <seed>` would read the new tree.
for _name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_PREFIX",
              "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES"):
    os.environ.pop(_name, None)


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True).stdout


def git_text(repo, *args):
    return git(repo, *args).decode().strip()


def common_dir(repo):
    return Path(git_text(repo, "rev-parse", "--path-format=absolute", "--git-common-dir"))


def configured_seed(repo):
    found = subprocess.run(["git", "-C", str(repo), "config", "--get", SEED_CONFIG],
                           capture_output=True, text=True)
    path = found.stdout.strip()
    return Path(path).resolve() if found.returncode == 0 and path else None


def seeds_dir(repo):
    """Locks and logs of the seeds, in the repository's git directory."""
    path = common_dir(repo) / "cranpose-seeds"
    path.mkdir(exist_ok=True)
    return path


@contextmanager
def seed_lock(repo, seed, mode):
    """Clones hold a seed's lock shared; retiring the seed takes it exclusive."""
    with open(seeds_dir(repo) / f"{seed.name}.lock", "w") as handle:
        try:
            fcntl.flock(handle, mode)
        except BlockingIOError:
            yield False
            return
        yield True


def committed_entries(repo):
    """Path -> (kind, object id) of every file and directory in HEAD, symlinks left out."""
    entries = {}
    for line in git(repo, "ls-tree", "-r", "-t", "-z", "HEAD").split(b"\0"):
        if line:
            meta, path = line.split(b"\t", 1)
            mode, kind, oid = meta.split(b" ")
            if mode != b"120000":
                entries[path] = (kind, oid)
    return entries


def edited_paths(repo):
    """Tracked files whose working copy differs from HEAD, and every directory above them."""
    edited = set()
    for path in git(repo, "diff", "--name-only", "-z", "HEAD", "--").split(b"\0"):
        while path:
            edited.add(path)
            path = os.path.dirname(path)
    return edited


def clone_tree(src, dst):
    if sys.platform == "darwin":
        command = ["cp", "-c", "-pR", str(src), str(dst)]
    else:
        command = ["cp", "-a", "--reflink=auto", str(src), str(dst)]
    subprocess.run(command, check=True)


def copy_mtimes(seed, tree):
    """Give every file and directory that is the same in both trees the seed's mtime.

    A directory counts as the same when its committed tree is equal and it
    holds the same names: build scripts watch directories, and cargo reads the
    directory entries' mtimes as well as the files'.
    """
    seed_entries = committed_entries(seed)
    edited = edited_paths(seed) | edited_paths(tree)
    seed_root, tree_root = os.fsencode(seed), os.fsencode(tree)
    copied = 0
    for path, entry in committed_entries(tree).items():
        if seed_entries.get(path) != entry or path in edited or entry[0] not in (b"blob", b"tree"):
            continue
        seed_path, tree_path = os.path.join(seed_root, path), os.path.join(tree_root, path)
        if entry[0] == b"tree" and sorted(os.listdir(seed_path)) != sorted(os.listdir(tree_path)):
            continue
        stat = os.stat(seed_path)
        os.utime(tree_path, ns=(stat.st_atime_ns, stat.st_mtime_ns))
        copied += 1
    return copied


# Cargo's fingerprint dep-info, format version 1 (cargo's fingerprint/dep_info.rs):
# a marker (u32 1, u8 0xff), u8 version, u32 file count, then per file u8 path
# type, u32-length path, u8 checksum flag [u64 size, u32-length checksum], then
# u32 env count and per entry u32-length key, u8 flag [u32-length value].
DEP_INFO_MARKER = b"\x01\x00\x00\x00\xff\x01"


def move_dep_info(data, seed_root, tree_root):
    """Point the files of a dep-info record at the tree.

    None when the record is not format version 1, or when its environment part
    names the seed: that value differs in the tree, so the unit must rebuild.
    """
    if not data.startswith(DEP_INFO_MARKER):
        return None
    out = bytearray(DEP_INFO_MARKER)
    pos = len(DEP_INFO_MARKER)

    def take(n):
        nonlocal pos
        chunk = data[pos:pos + n]
        if len(chunk) != n:
            raise ValueError("truncated dep-info")
        pos += n
        return chunk

    def take_bytes():
        return take(int.from_bytes(take(4), "little"))

    def put_bytes(value):
        out.extend(len(value).to_bytes(4, "little"))
        out.extend(value)

    try:
        count = take(4)
        out.extend(count)
        for _ in range(int.from_bytes(count, "little")):
            out.extend(take(1))
            path = take_bytes()
            if path == seed_root[:-1] or path.startswith(seed_root):
                path = tree_root[:-1] + path[len(seed_root) - 1:]
            put_bytes(path)
            has_checksum = take(1)
            out.extend(has_checksum)
            if has_checksum != b"\x00":
                out.extend(take(8))
                put_bytes(take_bytes())
    except ValueError:
        return None
    if seed_root[:-1] in data[pos:]:
        return None
    out.extend(data[pos:])
    return bytes(out)


def compiled_with_seed_path(profile_dir, seed_root):
    """Hashes of the units whose generated code may hold a path into the seed.

    rustc's own dep-info lists every variable a crate read with `env!`, and
    cargo leaves the ones it sets itself, CARGO_MANIFEST_DIR among them, out of
    its fingerprint. A unit that read one holding a seed path has that path in
    its code -- `#[composable]` bodies carry their crate's manifest dir -- so it
    rebuilds here. OUT_DIR moves with the target, and a unit that only checks
    (its one output is .rmeta) produces no code.
    """
    seed_target = seed_root + b"target/"
    hashes = set()
    for dep_info in [*profile_dir.glob("deps/*.d"), *profile_dir.glob("build/*/*.d")]:
        data = dep_info.read_bytes()
        unit_hash = dep_info.stem.rsplit("-", 1)[-1]
        # Each output is a rule of its own, and only outputs carry the unit's hash.
        outputs = [rule for rule in (line.split(b": ", 1)[0].rstrip(b":") for line in data.splitlines()
                                     if not line.startswith(b"#"))
                   if os.fsencode(f"-{unit_hash}") in os.path.basename(rule)]
        if all(output.endswith((b".rmeta", b".d")) for output in outputs):
            continue
        for line in data.splitlines():
            if not line.startswith(b"# env-dep:"):
                continue
            name, _, value = line[len(b"# env-dep:"):].partition(b"=")
            if seed_root[:-1] in value and not (name == b"OUT_DIR" and value.startswith(seed_target)):
                hashes.add(unit_hash)
                break
    return hashes


def move_fingerprints(target, seed, tree):
    """Point fingerprint records at the tree; drop the ones that cannot move."""
    seed_root = os.fsencode(seed) + b"/"
    tree_root = os.fsencode(tree) + b"/"
    moved = dropped = 0
    for fingerprints in [*target.glob("*/.fingerprint"), *target.glob("*/*/.fingerprint")]:
        profile_dir = fingerprints.parent
        holds_seed_path = compiled_with_seed_path(profile_dir, seed_root)
        for unit in list(fingerprints.iterdir()):
            rewrites = {}
            stale = unit.name.rsplit("-", 1)[-1] in holds_seed_path
            for record in [] if stale else unit.iterdir():
                data = record.read_bytes()
                if seed_root[:-1] not in data:
                    continue
                moved_data = move_dep_info(data, seed_root, tree_root) if record.name.startswith("dep-") else None
                if moved_data is None:
                    stale = True
                    break
                rewrites[record] = moved_data
            output = profile_dir / "build" / unit.name / "output"
            if not stale and output.is_file():
                # Paths into the cloned target move with it; a path into the
                # seed's sources would keep this build script watching the seed.
                original = output.read_bytes()
                data = original.replace(seed_root + b"target/", tree_root + b"target/")
                if seed_root[:-1] in data:
                    stale = True
                elif data != original:
                    rewrites[output] = data
            if stale:
                shutil.rmtree(unit)
                if output.is_file():
                    # Cargo fingerprints a build script that is about to run from
                    # its previous output's watch list; seed paths there would
                    # make the run's fingerprint change once more on the next build.
                    output.write_bytes(output.read_bytes().replace(seed_root[:-1], tree_root[:-1]))
                dropped += 1
                continue
            for record, data in rewrites.items():
                # Cargo reads a dep-info file's and a build script output's own
                # mtime as the time of the build they describe, so the rewrite
                # keeps it.
                times = record.stat()
                record.write_bytes(data)
                os.utime(record, ns=(times.st_atime_ns, times.st_mtime_ns))
                moved += 1
    return moved, dropped


def seed_tree(seed, tree):
    clone_tree(seed / "target", tree / "target")
    # The target sweep prunes incremental state no build touched for an hour;
    # a fresh clone's state is as current as the clone. rustc reads its
    # session names, not this mtime.
    for crate_dir in [*tree.glob("target/*/incremental/*"), *tree.glob("target/*/*/incremental/*")]:
        os.utime(crate_dir)
    copied = copy_mtimes(seed, tree)
    moved, dropped = move_fingerprints(tree / "target", seed, tree)
    print(f"seeded {tree}/target from {seed}: {copied} file times copied, "
          f"{moved} records moved, {dropped} units to rebuild")


def clone(args):
    tree = args.tree.resolve()
    if (tree / "target").exists():
        print(f"{tree}/target exists; nothing to seed")
        return
    seed = args.seed.resolve() if args.seed else configured_seed(tree)
    if seed is None:
        return
    with seed_lock(tree, seed, fcntl.LOCK_SH):
        if not (seed / "target").is_dir():
            print(f"seed {seed} has no target/; {tree} builds from nothing")
            return
        seed_tree(seed, tree)
    if args.refresh_behind:
        main = subprocess.run(["git", "-C", str(tree), "rev-parse", "--verify", "--quiet", "origin/main"],
                              capture_output=True, text=True).stdout.strip()
        if main and main != git_text(seed, "rev-parse", "HEAD"):
            start_background_refresh(tree)


def start_background_refresh(repo):
    log = seeds_dir(repo) / "refresh.log"
    with open(log, "a") as out:
        subprocess.Popen([sys.executable, __file__, "refresh", str(repo)], stdout=out, stderr=out,
                         stdin=subprocess.DEVNULL, start_new_session=True)
    print(f"seed refresh started in the background, log: {log}")


def seed_worktrees(repo):
    """Every worktree locked as a seed."""
    seeds, path = [], None
    for line in git_text(repo, "worktree", "list", "--porcelain").splitlines():
        if line.startswith("worktree "):
            path = Path(line[len("worktree "):])
        elif line == f"locked {SEED_LOCK_REASON}":
            seeds.append(path)
    return seeds


def refresh(args):
    repo = args.repo.resolve()
    primary = common_dir(repo).parent
    with open(seeds_dir(repo) / "refresh.lock", "w") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print("a seed refresh is already running")
            return
        subprocess.run(["git", "-C", str(repo), "fetch", "--quiet", "origin", "main"], check=True)
        commit = git_text(repo, "rev-parse", "origin/main")
        current = configured_seed(repo)
        if current and current.is_dir() and git_text(current, "rev-parse", "HEAD") == commit:
            print(f"seed {current} is at origin/main already")
            retire_old_seeds(repo, current)
            return
        seed = primary.parent / f"{primary.name}-seed-{commit[:10]}"
        if seed not in seed_worktrees(repo):
            # The hooks stay off: this seeds the worktree itself, and the hook
            # would start another refresh.
            subprocess.run(["git", "-C", str(repo), "-c", "core.hooksPath=/dev/null", "worktree", "add",
                            "--quiet", "--detach", str(seed), commit], check=True)
            subprocess.run(["git", "-C", str(repo), "worktree", "lock", "--reason", SEED_LOCK_REASON,
                            str(seed)], check=True)
            if current and current.is_dir():
                with seed_lock(repo, current, fcntl.LOCK_SH):
                    seed_tree(current, seed)
        os.nice(10)
        env = {key: value for key, value in os.environ.items()
               if key not in ("CARGO_TARGET_DIR", "RUSTC_WRAPPER", "CARGO_INCREMENTAL")}
        with open(seeds_dir(repo) / "warm.log", "w") as log:
            for command in [command.split() for command in args.warm] if args.warm else WARM_COMMANDS:
                started = time.monotonic()
                result = subprocess.run(command, cwd=seed, env=env, stdout=log, stderr=log)
                status = "ok" if result.returncode == 0 else f"exit {result.returncode}"
                print(f"warm {' '.join(command)}: {status} in {time.monotonic() - started:.0f} s", flush=True)
        subprocess.run(["git", "-C", str(repo), "config", SEED_CONFIG, str(seed)], check=True)
        print(f"seed is now {seed}")
        retire_old_seeds(repo, seed)


def retire_old_seeds(repo, current):
    """Remove every other seed that no clone is reading now; the next refresh retries the rest."""
    done = Path(__file__).resolve().parent / "worktree_done.sh"
    for old in seed_worktrees(repo):
        if old == current:
            continue
        with seed_lock(repo, old, fcntl.LOCK_EX | fcntl.LOCK_NB) as free:
            if not free:
                print(f"seed {old} is being cloned; it is removed by a later refresh")
                continue
            subprocess.run(["git", "-C", str(repo), "worktree", "unlock", str(old)], check=True)
            if subprocess.run([str(done), str(old)], check=False).returncode != 0:
                # Kept, and locked again so the target sweep leaves it alone.
                subprocess.run(["git", "-C", str(repo), "worktree", "lock", "--reason", SEED_LOCK_REASON,
                                str(old)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="subcommand", required=True)
    clone_parser = commands.add_parser("clone", help="seed a worktree that has no target/")
    clone_parser.add_argument("tree", type=Path)
    clone_parser.add_argument("--seed", type=Path, help=f"seed worktree (default: git config {SEED_CONFIG})")
    clone_parser.add_argument("--refresh-behind", action="store_true",
                              help="start a background refresh when origin/main has moved past the seed")
    clone_parser.set_defaults(run=clone)
    refresh_parser = commands.add_parser("refresh", help="make a seed at origin/main and retire the old one")
    refresh_parser.add_argument("repo", type=Path, nargs="?", default=Path("."))
    refresh_parser.add_argument("--warm", action="append",
                                help="a warm-up command, split on spaces; repeatable (default: what agents build)")
    refresh_parser.set_defaults(run=refresh)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
