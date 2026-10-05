import argparse
import urllib.request
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont


GOOGLE = "https://raw.githubusercontent.com/google/fonts/9710da1eacb3be272583c3224dcb70f9da6eadbb/ofl/"
CJK = "https://raw.githubusercontent.com/notofonts/noto-cjk/f8d157532fbfaeda587e826d4cd5b21a49186f7c/"
FONTS = [
    ("Arabic", GOOGLE + "notosansarabic/NotoSansArabic%5Bwdth,wght%5D.ttf", GOOGLE + "notosansarabic/OFL.txt"),
    ("Devanagari", GOOGLE + "notosansdevanagari/NotoSansDevanagari%5Bwdth,wght%5D.ttf", GOOGLE + "notosansdevanagari/OFL.txt"),
    ("CJK", CJK + "Sans/Variable/TTF/NotoSansCJKsc-VF.ttf", CJK + "Sans/LICENSE"),
]


def download(url, path):
    if not path.exists():
        with urllib.request.urlopen(url, timeout=30) as response:
            path.write_bytes(response.read())


def main():
    parser = argparse.ArgumentParser(description="Build optional Noto font packs for Cranpose.")
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--only", choices=[font[0] for font in FONTS])
    args = parser.parse_args()
    destination = Path(__file__).resolve().parents[1] / "assets"
    args.cache.mkdir(parents=True, exist_ok=True)
    for name, url, license_url in FONTS:
        if args.only and args.only != name:
            continue
        path = args.cache / (name + ".ttf")
        download(url, path)
        download(license_url, destination / (name + "-OFL.txt"))
        font = TTFont(path, recalcTimestamp=False)
        characters = set(font.getBestCmap())
        if "fvar" in font:
            axes = {
                axis.axisTag: axis.defaultValue
                for axis in font["fvar"].axes
                if axis.axisTag != "wght"
            }
            if axes:
                font = instantiateVariableFont(font, axes, inplace=True)
        if set(font.getBestCmap()) != characters:
            raise ValueError(f"{name} character coverage changed")
        if name == "CJK":
            import io

            buffer = io.BytesIO()
            font.save(buffer)
            data = buffer.getvalue()
            for index in range(3):
                chunk = data[len(data) * index // 3 : len(data) * (index + 1) // 3]
                chunk_path = destination.parents[1] / f"cranpose-fonts-cjk-data-{index + 1}" / "data.bin"
                chunk_path.write_bytes(chunk)
                print(f"{chunk_path}: {len(chunk)} bytes", flush=True)
            continue
        output = destination / {
            "Arabic": "NotoSansArabic-VF.ttf",
            "Devanagari": "NotoSansDevanagari-VF.ttf",
        }[name]
        font.save(output)
        print(f"{output.name}: {output.stat().st_size} bytes", flush=True)


if __name__ == "__main__":
    main()
