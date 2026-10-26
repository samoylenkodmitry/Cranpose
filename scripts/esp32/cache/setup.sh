#!/bin/sh
# Builds Espressif's QEMU with TCG plugins, which its release builds leave
# out, and the cache plugin cache.c beside this script, into
# ~/.local/esp-qemu-plugins. The same QEMU tag as scripts/esp32/setup.sh.
# Needs a C compiler, ninja, pkg-config, glib, pixman, libgcrypt and libslirp
# (Homebrew: brew install ninja pkgconf glib pixman libgcrypt libslirp).
set -eu

qemu_tag=esp-develop-9.2.2-20260417
source_dir="$HOME/.local/esp-qemu-src"
install_dir="$HOME/.local/esp-qemu-plugins"
here=$(cd "$(dirname "$0")" && pwd)

if [ ! -d "$source_dir" ]; then
    git clone --depth 1 --branch "$qemu_tag" https://github.com/espressif/qemu "$source_dir"
fi
if [ ! -x "$install_dir/bin/qemu-system-xtensa" ]; then
    mkdir -p "$source_dir/build-plugins"
    (cd "$source_dir/build-plugins" && ../configure --target-list=xtensa-softmmu \
        --enable-plugins --enable-gcrypt --enable-slirp --disable-gnutls --disable-docs \
        --disable-werror --disable-sdl --disable-cocoa --disable-gtk --disable-capstone \
        --disable-vnc --prefix="$install_dir")
    # The install puts the chips' ROMs where the binary looks for them.
    ninja -C "$source_dir/build-plugins" install
fi

case "$(uname -s)" in
    Darwin) link="-bundle -Wl,-undefined,dynamic_lookup" ;;
    *) link="-shared" ;;
esac
# shellcheck disable=SC2046,SC2086
cc -O2 -fPIC -I"$source_dir/include/qemu" $(pkg-config --cflags glib-2.0) $link \
    $(pkg-config --libs glib-2.0) -o "$install_dir/libcache.so" "$here/cache.c"
echo "QEMU with plugins and the cache plugin are in $install_dir"
