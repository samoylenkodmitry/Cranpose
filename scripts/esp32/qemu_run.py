#!/usr/bin/env python3
"""Boot an ESP32-S3 firmware in Espressif's QEMU and watch its serial log.

Builds the merged flash image with espflash, starts QEMU headless with the
octal PSRAM and the virtual RGB panel ESP-IDF's `idf.py qemu` uses, and streams
the serial log. It stops as soon as the log shows `--until` (success), a panic
(failure) or the timeout passes (failure). `--screenshot` saves the virtual
panel as PNG at the moment the run stops. `--window` shows the panel in a
window and runs until QEMU exits.
"""

import argparse
import collections
import os
import pathlib
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import struct
import zlib

PANIC_MARKERS = ("Guru Meditation", "panicked at", "abort() was called", "Backtrace:")


def find_qemu(explicit):
    if explicit:
        return explicit
    env = os.environ.get("ESP_QEMU")
    if env:
        return env
    on_path = shutil.which("qemu-system-xtensa")
    if on_path:
        return on_path
    local = pathlib.Path.home() / ".local/esp-qemu/qemu/bin/qemu-system-xtensa"
    if local.exists():
        return str(local)
    sys.exit("qemu-system-xtensa not found: run `just esp32-setup` or set ESP_QEMU")


def flash_image(elf, bootloader, partitions, flash_size, out):
    subprocess.run(
        [
            "espflash", "save-image", "--chip", "esp32s3", "--merge",
            "--flash-size", flash_size, "--bootloader", str(bootloader),
            "--partition-table", str(partitions), str(elf), str(out),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
    )


def efuse_image(out):
    """An ESP32-S3 eFuse dump for a calibrated chip of revision v0.3.

    QEMU keeps the blocks back to back: BLOCK0 takes 24 bytes and BLOCK1 24,
    so BLOCK2 starts at byte 48. WAFER_VERSION_MINOR_LO (BLOCK1 bits 114-116)
    is 3 and BLK_VERSION_MAJOR (BLOCK2 bits 128-129) is 1. With version 0
    ESP-IDF calibrates the ADC by itself at startup and waits forever for the
    ADC QEMU does not emulate.
    """
    data = bytearray(1024)
    data[24 + 114 // 8] |= 0b011 << (114 % 8)
    data[48 + 128 // 8] |= 0b01 << (128 % 8)
    out.write_bytes(bytes(data))


def monitor(path, *commands):
    """Runs QEMU monitor commands and returns their replies."""
    replies = b""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.connect(str(path))
        sock.settimeout(5.0)

        def read_prompt():
            received = b""
            while not received.endswith(b"(qemu) "):
                data = sock.recv(1 << 16)
                if not data:
                    break
                received += data
            return received

        read_prompt()
        for command in commands:
            sock.sendall(command.encode() + b"\n")
            if command == "quit":
                break
            replies += read_prompt()
    return replies.decode("utf-8", "replace")


def ppm_to_png(ppm, png):
    """Converts QEMU's binary PPM screendump to PNG."""
    data = ppm.read_bytes()
    fields = []
    position = 0
    while len(fields) < 4:
        while data[position:position + 1].isspace():
            position += 1
        if data[position:position + 1] == b"#":
            position = data.index(b"\n", position)
            continue
        end = position
        while not data[end:end + 1].isspace():
            end += 1
        fields.append(data[position:end])
        position = end
    width, height = int(fields[1]), int(fields[2])
    pixels = data[position + 1:]
    stride = width * 3
    raw = b"".join(b"\0" + pixels[row * stride:(row + 1) * stride] for row in range(height))

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    png.parent.mkdir(parents=True, exist_ok=True)
    png.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def pump_display(path, stop, out):
    """Refreshes the display about 30 times a second.

    The virtual panel copies a frame only when a display refreshes, and
    `-display none` never refreshes. Each screendump refreshes it once, and
    the panel driver's draw call waits for that copy.
    """
    while not stop.is_set():
        try:
            monitor(path, f"screendump {out}")
        except OSError:
            pass
        stop.wait(0.03)


def find_addr2line():
    tools = pathlib.Path.home() / ".espressif/tools/xtensa-esp-elf"
    found = sorted(tools.glob("*/xtensa-esp-elf/bin/xtensa-esp-elf-addr2line"))
    return str(found[-1]) if found else shutil.which("xtensa-esp-elf-addr2line")


def print_cpu_samples(path, elf, samples=20):
    """Samples both CPUs' program counters and names the functions they ran."""
    counts = collections.Counter()
    for _ in range(samples):
        replies = monitor(path, "cpu 0", "info registers", "cpu 1", "info registers")
        for cpu, pc in enumerate(re.findall(r"\bPC=([0-9a-fA-F]+)", replies)):
            counts[(cpu, pc)] += 1
    addr2line = find_addr2line()
    names = {}
    if addr2line and counts:
        pcs = sorted({pc for _, pc in counts})
        output = subprocess.run(
            [addr2line, "-fiaC", "-e", str(elf), *(f"0x{pc}" for pc in pcs)],
            capture_output=True, text=True, check=False,
        ).stdout
        current = None
        for line in output.splitlines():
            if line.startswith("0x"):
                current = line[2:].lstrip("0") or "0"
            elif current is not None and current not in names:
                names[current] = line
    for (cpu, pc), count in counts.most_common(12):
        name = names.get(pc.lstrip("0") or "0", "?")
        print(f"qemu_run: cpu {cpu} {count:2}x {pc} {name}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", required=True, type=pathlib.Path)
    parser.add_argument("--bootloader", type=pathlib.Path,
                        help="defaults to the bootloader.bin beside the ELF")
    parser.add_argument("--partitions", required=True, type=pathlib.Path)
    parser.add_argument("--flash-size", default="8mb")
    parser.add_argument("--psram", default="8M")
    parser.add_argument("--qemu")
    parser.add_argument("--until", help="log text that ends the run successfully")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--screenshot", type=pathlib.Path)
    parser.add_argument("--log", type=pathlib.Path)
    parser.add_argument("--window", action="store_true")
    args = parser.parse_args()

    workdir = pathlib.Path(tempfile.mkdtemp(prefix="esp32-qemu-"))
    image = workdir / "flash.bin"
    bootloader = args.bootloader or args.elf.parent / "bootloader.bin"
    flash_image(args.elf, bootloader, args.partitions, args.flash_size, image)
    efuse = workdir / "efuse.bin"
    efuse_image(efuse)
    monitor_path = workdir / "monitor.sock"
    pump_path = workdir / "pump.sock"
    serial = workdir / "serial.log"
    command = [
        find_qemu(args.qemu),
        "-M", "esp32s3", "-m", args.psram,
        "-drive", f"file={image},if=mtd,format=raw",
        "-drive", f"file={efuse},if=none,format=raw,id=efuse",
        "-global", "driver=nvram.esp32s3.efuse,property=drive,value=efuse",
        "-global", "driver=timer.esp32s3.timg,property=wdt_disable,value=true",
        "-global", "driver=ssi_psram,property=is_octal,value=true",
        "-nic", "user,model=open_eth",
        "-display", "cocoa" if args.window and sys.platform == "darwin" else ("sdl" if args.window else "none"),
        "-serial", f"file:{serial}",
        "-monitor", f"unix:{monitor_path},server,nowait",
    ]
    if not args.window:
        command += ["-monitor", f"unix:{pump_path},server,nowait"]
    log = args.log.open("w") if args.log else None
    qemu = subprocess.Popen(command, stdin=subprocess.DEVNULL)
    stop_pump = threading.Event()
    pump = None
    if not args.window:
        while not pump_path.exists() and qemu.poll() is None:
            time.sleep(0.05)
        pump = threading.Thread(
            target=pump_display, args=(pump_path, stop_pump, workdir / "refresh.ppm"), daemon=True
        )
        pump.start()
    deadline = None if args.window else time.monotonic() + args.timeout
    outcome = None
    pending = b""
    offset = 0
    while outcome is None:
        if deadline is not None and time.monotonic() >= deadline:
            outcome = "timeout"
            break
        chunk = b""
        if serial.exists():
            with serial.open("rb") as handle:
                handle.seek(offset)
                chunk = handle.read()
            offset += len(chunk)
        if not chunk:
            if qemu.poll() is not None:
                outcome = "exited"
            time.sleep(0.1)
            continue
        pending += chunk
        *lines, pending = pending.split(b"\n")
        for raw in lines:
            line = raw.decode("utf-8", "replace").rstrip("\r")
            print(line, flush=True)
            if log:
                log.write(line + "\n")
            if any(marker in line for marker in PANIC_MARKERS):
                outcome = "panic"
            elif args.until and args.until in line:
                outcome = "done"
    stop_pump.set()
    if pump:
        pump.join()
    if outcome == "timeout" and qemu.poll() is None:
        print_cpu_samples(monitor_path, args.elf)
    if args.screenshot and qemu.poll() is None:
        dump = workdir / "screen.ppm"
        monitor(monitor_path, f"screendump {dump}")
        if dump.exists():
            ppm_to_png(dump, args.screenshot)
        else:
            print("qemu_run: the screendump wrote no file", file=sys.stderr)
    if qemu.poll() is None:
        monitor(monitor_path, "quit")
    try:
        qemu.wait(timeout=10)
    except subprocess.TimeoutExpired:
        qemu.kill()
    if log:
        log.close()
    shutil.rmtree(workdir, ignore_errors=True)
    print(f"qemu_run: {outcome}", file=sys.stderr)
    success = outcome == "done" or (outcome == "exited" and args.window)
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
