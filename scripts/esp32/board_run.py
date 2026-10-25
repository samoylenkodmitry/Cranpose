#!/usr/bin/env python3
"""Builds an app for a board, flashes it and saves its serial log.

The apps are the Cranpose demo (apps/esp32-demo) and the LVGL benchmark
(benchmarks/esp32/lvgl-app). `--component` links an ESP-IDF component into
either, with the settings of its sdkconfig.defaults, such as
scripts/esp32/sampler or benchmarks/esp32/perfstats. The ELF
is copied beside the log, so a profile in the log can be read later.

Run it in the Xtensa Rust environment (`. ~/export-esp-1.98.1.sh`); the LVGL
build takes ESP-IDF from ~/.espressif, where the demo's first build put it.
"""

import argparse
import fcntl
import glob
import os
import pathlib
import pty
import re
import select
import shutil
import signal
import subprocess
import sys
import termios
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEMO = ROOT / "apps/esp32-demo"
LVGL = ROOT / "benchmarks/esp32/lvgl-app"
ESPRESSIF = pathlib.Path.home() / ".espressif"
IDF_VERSION = "v5.5.5"

BOARDS = {
    "es3c35p": {"mcu": "esp32s3", "console": "usb", "port": "/dev/cu.usbmodem*",
                "baud": 921600, "partitions": "partitions.csv"},
    "e32r35t": {"mcu": "esp32", "console": "uart", "port": "/dev/cu.usbserial-*",
                "baud": 460800, "partitions": "partitions_4mb.csv"},
}
SUMMARY = re.compile(r"first frame|frames in|PERF|HEAP|SAMPLER total|panic|abort\(\)|memory allocation")


def run(command, cwd, env, log):
    with open(log, "w") as out:
        if subprocess.run(command, cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT).returncode:
            sys.exit(f"{' '.join(map(str, command[:3]))} ... failed: see {log}")


def one(pattern, what):
    found = sorted(glob.glob(pattern))
    if len(found) != 1:
        sys.exit(f"{what}: {len(found)} matches for {pattern}; pass it explicitly")
    return found[0]


def build_demo(board, component, extra_sdkconfig, features, log):
    config = BOARDS[board]
    target = f"xtensa-{config['mcu']}-espidf"
    defaults = ";".join([f"sdkconfig.defaults;sdkconfig.{board}", *map(str, extra_sdkconfig)])
    env = dict(os.environ, MCU=config["mcu"], ESP_IDF_SDKCONFIG_DEFAULTS=defaults)
    if component:
        extra = log.with_suffix(".components.toml")
        extra.write_text(f'[[extra_components]]\ncomponent_dirs = ["{component}"]\n')
        env["ESP_IDF_SYS_EXTRA_COMPONENTS_FILE"] = str(extra)
    # esp-idf-sys rebuilds its CMake project only when its own inputs change,
    # so a component added or removed since the last build needs a nudge.
    (DEMO / "sdkconfig.defaults").touch()
    run(["cargo", "build", "--release", "--target", target, "--features", ",".join([board, *features])],
        DEMO, env, log.with_suffix(".build"))
    release = DEMO / "target" / target / "release"
    return release / "esp32-demo", release / "bootloader.bin", DEMO / config["partitions"]


def idf_environment():
    env = dict(os.environ)
    idf = ESPRESSIF / "esp-idf" / IDF_VERSION
    tools = ESPRESSIF / "tools"
    paths = [
        # ESP-IDF keeps an environment for each Python that installed it:
        # the one of the Python running this script.
        one(str(ESPRESSIF / f"python_env/idf*_py{sys.version_info.major}.{sys.version_info.minor}_env/bin"),
            "ESP-IDF Python"),
        one(str(tools / "xtensa-esp-elf/*/xtensa-esp-elf/bin"), "Xtensa GCC"),
        one(str(tools / "cmake/*/CMake.app/Contents/bin"), "CMake")
        if sys.platform == "darwin" else one(str(tools / "cmake/*/bin"), "CMake"),
        one(str(tools / "ninja/*"), "Ninja"),
    ]
    env.update(IDF_PATH=str(idf), IDF_TOOLS_PATH=str(ESPRESSIF),
               ESP_ROM_ELF_DIR=one(str(tools / "esp-rom-elfs/*"), "ROM ELFs") + "/",
               PATH=os.pathsep.join(paths + [env["PATH"]]))
    return env


def build_lvgl(board, component, extra_sdkconfig, log):
    config = BOARDS[board]
    mcu = config["mcu"]
    env = idf_environment()
    demo = os.path.relpath(DEMO, LVGL)
    defaults = ";".join([f"sdkconfig.lvgl;{demo}/sdkconfig.defaults;{demo}/sdkconfig.{board}",
                         *map(str, extra_sdkconfig)])
    command = ["python", str(ESPRESSIF / "esp-idf" / IDF_VERSION / "tools/idf.py"),
               "-B", f"build-{mcu}", "-D", f"SDKCONFIG=build-{mcu}/sdkconfig",
               "-D", f"SDKCONFIG_DEFAULTS={defaults}",
               "-D", f"EXTRA_COMPONENT_DIRS={component or ''}"]
    if not (LVGL / f"build-{mcu}/CMakeCache.txt").exists():
        command.append(f"-DIDF_TARGET={mcu}")
    # ESP-IDF reads the defaults only into a new sdkconfig, so the build
    # writes its sdkconfig from them every time, as esp-idf-sys does for the
    # demo.
    (LVGL / f"build-{mcu}/sdkconfig").unlink(missing_ok=True)
    run(command + ["build"], LVGL, env, log.with_suffix(".build"))
    build = LVGL / f"build-{mcu}"
    return build / "lvgl_bench.elf", build / "bootloader/bootloader.bin", DEMO / config["partitions"]


def flash(board, port, elf, bootloader, partitions, log):
    # The bootloader ESP-IDF built turns QIO on; the header stays DIO, as
    # ESP-IDF writes it for a QIO app.
    run(["espflash", "flash", "--port", port, "--baud", str(BOARDS[board]["baud"]),
         "--flash-mode", "dio", "--flash-freq", "80mhz", "--bootloader", str(bootloader),
         "--partition-table", str(partitions), str(elf)], ROOT, None, log.with_suffix(".flash"))


def capture_uart(port, seconds):
    """Resets a board through the DTR and RTS lines of its USB-UART bridge and
    reads its UART for `seconds`."""
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    attrs[0:4] = [0, 0, termios.CS8 | termios.CREAD | termios.CLOCAL, 0]
    attrs[4] = attrs[5] = termios.B115200
    termios.tcsetattr(fd, termios.TCSANOW, attrs)

    def lines(dtr, rts):
        on = (termios.TIOCM_DTR if dtr else 0) | (termios.TIOCM_RTS if rts else 0)
        off = (termios.TIOCM_DTR | termios.TIOCM_RTS) & ~on
        for request, mask in ((termios.TIOCMBIS, on), (termios.TIOCMBIC, off)):
            if mask:
                fcntl.ioctl(fd, request, mask.to_bytes(4, "little"))

    # EN follows RTS inverted, IO0 follows DTR inverted: hold IO0 high and
    # pulse EN.
    lines(False, True)
    time.sleep(0.2)
    lines(False, False)
    return read_for(fd, seconds, lambda: os.close(fd))


def capture_usb(port, seconds):
    """Reads the USB Serial/JTAG console through `espflash monitor`, which
    resets the chip on Ctrl+R."""
    pid, fd = pty.fork()
    if pid == 0:
        os.execvp("espflash", ["espflash", "monitor", "--port", port])
    time.sleep(1.5)
    os.write(fd, b"\x12")

    def stop():
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)

    return read_for(fd, seconds, stop)


def read_for(fd, seconds, close):
    deadline = time.time() + seconds
    out = bytearray()
    while time.time() < deadline:
        ready, _, _ = select.select([fd], [], [], 0.5)
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            out += chunk
    close()
    return out.decode("utf-8", "replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--board", choices=BOARDS, required=True)
    parser.add_argument("--app", choices=["demo", "lvgl"], default="demo")
    parser.add_argument("--component", type=pathlib.Path,
                        help="an ESP-IDF component directory to link in")
    parser.add_argument("--sdkconfig", type=pathlib.Path, action="append", default=[],
                        help="an sdkconfig defaults file to add after the board's")
    parser.add_argument("--feature", action="append", default=[],
                        help="a feature of the demo to build it with, such as text-drawn")
    parser.add_argument("--port", help="the serial port; found by name when omitted")
    parser.add_argument("--seconds", type=float, default=17)
    parser.add_argument("--log", type=pathlib.Path, required=True)
    args = parser.parse_args()

    config = BOARDS[args.board]
    # The builds run in the apps' directories.
    args.log = args.log.resolve()
    port = args.port or one(config["port"], "serial port")
    component = args.component.resolve() if args.component else None
    args.log.parent.mkdir(parents=True, exist_ok=True)
    extra = [path.resolve() for path in args.sdkconfig]
    # A component's own settings, such as the run time counter perfstats
    # reads, come before the ones the command line adds.
    if component and (component / "sdkconfig.defaults").exists():
        extra.insert(0, component / "sdkconfig.defaults")
    if args.app == "demo":
        elf, bootloader, partitions = build_demo(args.board, component, extra, args.feature, args.log)
    elif args.feature:
        sys.exit("--feature builds the demo; the LVGL app takes its options from --sdkconfig")
    else:
        elf, bootloader, partitions = build_lvgl(args.board, component, extra, args.log)
    flash(args.board, port, elf, bootloader, partitions, args.log)
    capture = capture_usb if config["console"] == "usb" else capture_uart
    text = capture(port, args.seconds)
    args.log.write_text(text)
    shutil.copy(elf, args.log.with_suffix(".elf"))
    for line in text.splitlines():
        if SUMMARY.search(line):
            print(line.strip())


main()
