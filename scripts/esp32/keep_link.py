#!/usr/bin/env python3
"""A linker for cargo that keeps the link it runs, for board_ab.py to run
again with another order file without building again.

It copies the objects and archives the link names by path into
$KEEP_LINK_DIR/files, so a later build cannot change them, writes the
arguments with those copies to $KEEP_LINK_DIR/args.json and links with
ldproxy, the demo's linker. ESP-IDF's libraries come from its build
directory by name, so a relink must run before another build of the same
tree changes them.
"""

import json
import os
import pathlib
import shutil
import subprocess
import sys

keep = pathlib.Path(os.environ["KEEP_LINK_DIR"])
files = keep / "files"
files.mkdir(parents=True, exist_ok=True)
copies = {}


def kept(argument):
    path = pathlib.Path(argument)
    if path.suffix not in (".o", ".rlib", ".a") or not path.is_file():
        return argument
    if argument not in copies:
        copy = files / f"{len(copies)}-{path.name}"
        shutil.copyfile(argument, copy)
        copies[argument] = str(copy)
    return copies[argument]


def expanded(arguments):
    """The arguments with each response file rustc passed read in."""
    out = []
    for argument in arguments:
        if argument.startswith("@") and pathlib.Path(argument[1:]).is_file():
            out += expanded(line for line in pathlib.Path(argument[1:]).read_text().splitlines() if line)
        else:
            out.append(argument)
    return out


arguments = [kept(argument) for argument in expanded(sys.argv[1:])]
(keep / "args.json").write_text(json.dumps({"cwd": os.getcwd(), "args": arguments}))
sys.exit(subprocess.run(["ldproxy", *arguments]).returncode)
