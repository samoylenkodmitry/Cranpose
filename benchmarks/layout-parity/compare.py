#!/usr/bin/env python3
"""Run and check Jetpack Compose vs Cranpose layout and behavioral parity.

Usage:
    python3 benchmarks/layout-parity/compare.py --run --check-ledger --out target/layout-parity
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
REPO_ROOT = ROOT.parent.parent
DEFAULT_LEDGER = ROOT / "divergences.toml"

ZONE_NAMES = {
    "root": "00. Root and Chrome",
    "header": "00. Root and Chrome",
    "hdr": "00. Root and Chrome",
    "footer": "00. Root and Chrome",
    "ftr": "00. Root and Chrome",
    "main": "00. Root and Chrome",
    "col": "00. Root and Chrome",
    "z1": "01. Modifier Chain, Coercion, requiredSize, sizeIn, wrapContent",
    "z2": "02. Box 9-Align, propagateMinConstraints, zIndex",
    "z3": "03. Linear Arrangements, SpacedByAligned, Overflow Clamp",
    "z4": "04. Flex Weights, Mixed Fixed, Tight Remainder",
    "z5": "05. Intrinsics (Min/Max Width and Height, Nested)",
    "z6": "06. Baselines and Text Mechanics",
    "z7": "07. Custom Layout, Modifier.layout, BoxWithConstraints",
    "z8": "08. SubcomposeLayout (Two-Pass and Dynamic Slots)",
    "z9": "09. RTL vs LTR Mirroring, Relative Padding/Offset, LTR Island",
    "z10": "10. FlowRow and FlowColumn",
    "z11": "11. Offsets and GraphicsLayer",
    "z12": "12. Draw Modifiers, Shapes, Shadow",
    "z13": "13. verticalScroll, horizontalScroll, LazyRow(reverse)",
    "z14": "14. LazyColumn (contentPadding and stickyHeader)",
    "z15": "15. LazyVerticalGrid and LazyVerticalStaggeredGrid",
    "z16": "16. HorizontalPager and 10-Level Deep Nesting",
    "z17": "17. LazyRow in LazyColumn (With/Without height and Perms)",
    "z18": "18. Wrapped Scroll/Flow/BWC/Intr/Pager in Lazy and VScroll",
    "z19": "19. LazyCol/VScroll in LazyRow, AdaptiveGrid in BWC, Stag in Row",
    "z20": "20. Modifier Order Permutation Matrix",
}

VALID_KINDS = {"bounds_delta", "non_finite", "missing_in_cranpose", "missing_in_compose"}


def density_dir_name(density_label: str) -> str:
    return "d" + density_label.replace(".", "_")


def zone_of(tag: str) -> str:
    return ZONE_NAMES.get(tag.split("_", 1)[0], "99. Other")


def load_ledger(path: Path) -> dict[str, Any]:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def build_and_run_apps(
    out_dir: Path,
    width_dp: int,
    height_dp: int,
    densities: list[str],
    cranpose_profile: str,
) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    compose_stress = out_dir / "compose_stress.json"
    gradlew = REPO_ROOT / "tools" / "compose-twin" / "gradlew"
    compose_args = (
        f"--headless {out_dir.resolve()} "
        f"--width {width_dp} --height {height_dp} "
        f"--densities {','.join(densities)} "
        f"--stress-json {compose_stress.resolve()}"
    )
    compose_cmd = [
        str(gradlew),
        "-p",
        str(ROOT / "compose-app"),
        "--no-daemon",
        "run",
        f"--args={compose_args}",
        "--console=plain",
    ]
    print(f"[run] Compose Desktop ({', '.join(densities)}x + stress) -> {out_dir}", flush=True)
    subprocess.run(compose_cmd, cwd=REPO_ROOT, check=True)

    manifest = str(ROOT / "cranpose-app" / "Cargo.toml")
    profile_args = ["--release"] if cranpose_profile == "release" else []
    print(f"[build] Cranpose ({cranpose_profile})", flush=True)
    subprocess.run(["cargo", "build", "--manifest-path", manifest, *profile_args], cwd=REPO_ROOT, check=True)

    for label in densities:
        d_out = out_dir / density_dir_name(label)
        d_out.mkdir(parents=True, exist_ok=True)
        print(f"[run] Cranpose {label}x -> {d_out}", flush=True)
        subprocess.run(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                manifest,
                *profile_args,
                "--",
                f"--headless={d_out.resolve()}",
                f"--width={width_dp}",
                f"--height={height_dp}",
                f"--density={label}",
            ],
            cwd=REPO_ROOT,
            check=True,
        )

    cranpose_stress = out_dir / "cranpose_stress.json"
    print(f"[run] Cranpose stress suite -> {cranpose_stress}", flush=True)
    proc = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            manifest,
            *profile_args,
            "--",
            "--stress",
            f"--stress-json={cranpose_stress.resolve()}",
        ],
        cwd=REPO_ROOT,
        check=False,
    )
    if not cranpose_stress.exists():
        raise RuntimeError(f"Cranpose stress run exited with {proc.returncode} without writing {cranpose_stress}")


def parse_rect(raw: dict[str, Any]) -> tuple[dict[str, float], bool]:
    rect = {k: float(raw[k]) for k in ("x", "y", "width", "height")}
    return rect, any(not math.isfinite(v) for v in rect.values())


def compare_tag(tag: str, raw_c: dict[str, Any], raw_r: dict[str, Any]) -> dict[str, Any]:
    rect_c, bad_c = parse_rect(raw_c)
    rect_r, bad_r = parse_rect(raw_r)
    if bad_c or bad_r:
        return {
            "tag": tag,
            "zone": zone_of(tag),
            "compose": {k: (v if math.isfinite(v) else str(v)) for k, v in rect_c.items()},
            "cranpose": {k: (v if math.isfinite(v) else str(v)) for k, v in rect_r.items()},
            "dx": None,
            "dy": None,
            "dw": None,
            "dh": None,
            "max_delta": None,
            "non_finite": True,
        }
    dx = rect_r["x"] - rect_c["x"]
    dy = rect_r["y"] - rect_c["y"]
    dw = rect_r["width"] - rect_c["width"]
    dh = rect_r["height"] - rect_c["height"]
    return {
        "tag": tag,
        "zone": zone_of(tag),
        "compose": rect_c,
        "cranpose": rect_r,
        "dx": dx,
        "dy": dy,
        "dw": dw,
        "dh": dh,
        "max_delta": max(abs(dx), abs(dy), abs(dw), abs(dh)),
        "non_finite": False,
    }


def summarize_zones(entries: list[dict[str, Any]], tol_dp: float) -> list[dict[str, Any]]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for entry in entries:
        grouped[entry["zone"]].append(entry)
    rows: list[dict[str, Any]] = []
    for zone in sorted(grouped):
        items = grouped[zone]
        non_finite = sum(1 for e in items if e["non_finite"])
        finite_bad = sum(1 for e in items if not e["non_finite"] and e["max_delta"] > tol_dp)
        finite_deltas = [e["max_delta"] for e in items if not e["non_finite"]]
        rows.append(
            {
                "zone": zone,
                "total": len(items),
                "non_finite": non_finite,
                "mismatches": finite_bad + non_finite,
                "max_delta": None if non_finite else max(finite_deltas, default=0.0),
            }
        )
    return rows


def compare_density_bounds(density_label: str, d_dir: Path, tol_dp: float) -> dict[str, Any]:
    compose_data = json.loads((d_dir / "compose_bounds.json").read_text(encoding="utf-8"))
    cranpose_data = json.loads((d_dir / "cranpose_bounds.json").read_text(encoding="utf-8"))
    c_nodes = compose_data["nodes"]
    r_nodes = cranpose_data["nodes"]

    only_compose = sorted(set(c_nodes) - set(r_nodes))
    only_cranpose = sorted(set(r_nodes) - set(c_nodes))
    common = sorted(set(c_nodes) & set(r_nodes))

    entries = [compare_tag(tag, c_nodes[tag], r_nodes[tag]) for tag in common]
    exact = sum(1 for e in entries if not e["non_finite"] and e["max_delta"] <= 0.01)
    within_tol = sum(1 for e in entries if not e["non_finite"] and e["max_delta"] <= tol_dp)
    mismatches = [e for e in entries if e["non_finite"] or e["max_delta"] > tol_dp]
    non_finite_tags = [e["tag"] for e in entries if e["non_finite"]]

    return {
        "density_label": density_label,
        "width_dp": compose_data["width_dp"],
        "height_dp": compose_data["height_dp"],
        "density": compose_data["density"],
        "tol_dp": tol_dp,
        "compose_count": len(c_nodes),
        "cranpose_count": len(r_nodes),
        "common_count": len(common),
        "exact_count": exact,
        "within_tol_count": within_tol,
        "non_finite_tags": non_finite_tags,
        "only_compose": only_compose,
        "only_cranpose": only_cranpose,
        "all_tags": sorted(set(c_nodes) | set(r_nodes)),
        "entries": {e["tag"]: e for e in entries},
        "mismatches": mismatches,
        "zones": summarize_zones(entries, tol_dp),
    }


def compare_images(d_dir: Path) -> dict[str, Any] | None:
    try:
        from PIL import Image, ImageChops
    except ImportError:
        return None

    c_path = d_dir / "compose.png"
    r_path = d_dir / "cranpose.png"
    if not (c_path.exists() and r_path.exists()):
        return None

    img_c = Image.open(c_path).convert("RGB")
    img_r = Image.open(r_path).convert("RGB")
    w = min(img_c.width, img_r.width)
    h = min(img_c.height, img_r.height)
    img_c = img_c.crop((0, 0, w, h))
    img_r = img_r.crop((0, 0, w, h))

    raw = ImageChops.difference(img_c, img_r).tobytes()
    c_bytes = img_c.tobytes()
    h_bytes = bytearray(len(raw))
    changed_8 = 0
    changed_24 = 0
    sum_delta = 0
    for i in range(0, len(raw), 3):
        peak = max(raw[i], raw[i + 1], raw[i + 2])
        sum_delta += raw[i] + raw[i + 1] + raw[i + 2]
        if peak > 24:
            changed_24 += 1
            changed_8 += 1
            h_bytes[i : i + 3] = b"\xff\x14\x5a"
        elif peak > 8:
            changed_8 += 1
            h_bytes[i : i + 3] = b"\xff\xaf\x00"
        else:
            h_bytes[i] = c_bytes[i] // 3 + 170
            h_bytes[i + 1] = c_bytes[i + 1] // 3 + 170
            h_bytes[i + 2] = c_bytes[i + 2] // 3 + 170

    Image.frombytes("RGB", (w, h), bytes(h_bytes)).save(d_dir / "diff_heatmap.png")
    gap = 8
    side = Image.new("RGB", (w * 2 + gap, h), (30, 41, 59))
    side.paste(img_c, (0, 0))
    side.paste(img_r, (w + gap, 0))
    side.save(d_dir / "side_by_side.png")

    total_px = max(1, w * h)
    return {
        "width_px": w,
        "height_px": h,
        "mean_rgb_delta": round(sum_delta / (total_px * 3), 3),
        "pct_gt_8": round(100.0 * changed_8 / total_px, 3),
        "pct_gt_24": round(100.0 * changed_24 / total_px, 3),
    }


def index_layout_rules(ledger: dict[str, Any]) -> tuple[dict[tuple[str, str], dict[str, Any]], list[str]]:
    errors: list[str] = []
    rules: dict[tuple[str, str], dict[str, Any]] = {}
    layout = ledger.get("layout", {})
    for category in ("expected", "unexpected"):
        for item in layout.get(category, []):
            kind = item.get("kind", "bounds_delta")
            if kind not in VALID_KINDS:
                errors.append(f"ledger rule '{item.get('id')}' has invalid kind '{kind}'")
                continue
            if category == "unexpected" and not item.get("issue"):
                errors.append(f"unexpected ledger rule '{item.get('id')}' must declare an issue")
            rule = {**item, "category": category, "kind": kind}
            for density in item.get("densities", []):
                for tag in item.get("tags", []):
                    key = (str(density), tag)
                    if key in rules:
                        errors.append(
                            f"duplicate ledger entry for ({density}x, {tag}): "
                            f"'{rules[key]['id']}' and '{rule['id']}'"
                        )
                    rules[key] = rule
    return rules, errors


def observed_tag_state(report: dict[str, Any], tag: str) -> tuple[str, float | None]:
    if tag in report["only_compose"]:
        return "missing_in_cranpose", None
    if tag in report["only_cranpose"]:
        return "missing_in_compose", None
    entry = report["entries"].get(tag)
    if entry is None:
        return "absent", None
    if entry["non_finite"]:
        return "non_finite", None
    if entry["max_delta"] > report["tol_dp"]:
        return "bounds_delta", float(entry["max_delta"])
    return "pass", float(entry["max_delta"])


def check_density_against_ledger(
    report: dict[str, Any],
    rules: dict[tuple[str, str], dict[str, Any]],
) -> tuple[list[str], dict[str, int]]:
    density = report["density_label"]
    errors: list[str] = []
    counts = {"expected": 0, "unexpected": 0}
    seen_tags = set(report["all_tags"])

    for (rule_density, tag), rule in sorted(rules.items()):
        if rule_density != density:
            continue
        if tag not in seen_tags:
            errors.append(f"[{density}x] ledger tag '{tag}' (rule '{rule['id']}') is not in atlas output")

    for tag in sorted(seen_tags):
        state, delta = observed_tag_state(report, tag)
        rule = rules.get((density, tag))
        if state == "pass":
            if rule and rule["category"] == "unexpected":
                errors.append(
                    f"[{density}x] fixed unexpected bug: '{tag}' now passes (delta={delta:.2f}dp); "
                    f"remove from rule '{rule['id']}' ({rule['issue']}) and update docs/compose_api_parity.md"
                )
            continue
        if rule is None:
            detail = f"delta={delta:.2f}dp" if delta is not None else state
            errors.append(f"[{density}x] unlisted layout divergence: '{tag}' ({detail})")
            continue
        if rule["kind"] != state:
            errors.append(
                f"[{density}x] ledger kind mismatch for '{tag}' (rule '{rule['id']}'): "
                f"expected '{rule['kind']}', observed '{state}'"
            )
            continue
        if state == "bounds_delta" and delta is not None and delta > float(rule.get("max_dp", 0.0)):
            errors.append(
                f"[{density}x] regression on '{tag}' (rule '{rule['id']}'): "
                f"delta={delta:.2f}dp exceeds max_dp={float(rule['max_dp']):.2f}dp"
            )
            continue
        counts[rule["category"]] += 1
    return errors, counts


def check_stress_against_ledger(
    ledger: dict[str, Any],
    stress_reports: dict[str, dict[str, Any]],
) -> tuple[list[str], int]:
    errors: list[str] = []
    expected_failures: dict[tuple[str, str], dict[str, Any]] = {}
    for item in ledger.get("stress", {}).get("unexpected", []):
        if not item.get("issue"):
            errors.append(f"stress rule '{item.get('scenario')}' must declare an issue")
        key = (item["framework"], item["scenario"])
        if key in expected_failures:
            errors.append(f"duplicate stress ledger entry for {key}")
        expected_failures[key] = item

    seen_keys: set[tuple[str, str]] = set()
    tracked_failures = 0
    for framework, report in sorted(stress_reports.items()):
        for scenario in report.get("scenarios", []):
            key = (framework, scenario["name"])
            seen_keys.add(key)
            rule = expected_failures.get(key)
            if scenario["passed"]:
                if rule is not None:
                    errors.append(
                        f"[stress:{framework}] fixed unexpected panic: '{scenario['name']}' now passes; "
                        f"remove from divergences.toml ({rule['issue']}) and update docs/compose_api_parity.md"
                    )
            else:
                if rule is None:
                    errors.append(
                        f"[stress:{framework}] unlisted stress failure: '{scenario['name']}': "
                        f"{scenario.get('panic') or 'failed'}"
                    )
                else:
                    tracked_failures += 1

    for key, rule in sorted(expected_failures.items()):
        if key not in seen_keys:
            errors.append(f"[stress:{key[0]}] ledger scenario '{key[1]}' ({rule['issue']}) did not run")
    return errors, tracked_failures


def evaluate_ledger(
    ledger: dict[str, Any],
    density_reports: list[dict[str, Any]],
    stress_reports: dict[str, dict[str, Any]],
) -> dict[str, Any]:
    rules, errors = index_layout_rules(ledger)
    per_density: dict[str, dict[str, int]] = {}
    for report in density_reports:
        d_errors, counts = check_density_against_ledger(report, rules)
        errors.extend(d_errors)
        per_density[report["density_label"]] = counts
    stress_errors, tracked_stress = check_stress_against_ledger(ledger, stress_reports)
    errors.extend(stress_errors)
    return {
        "ok": len(errors) == 0,
        "errors": errors,
        "layout_counts": per_density,
        "tracked_stress_failures": tracked_stress,
    }


def render_markdown_summary(
    density_reports: list[dict[str, Any]],
    stress_reports: dict[str, dict[str, Any]],
    evaluation: dict[str, Any] | None,
) -> str:
    lines = ["## Layout and Behavioral Parity Report", ""]
    lines.append("| Density | Tracked Nodes | Exact (<=0.01dp) | Within 0.5dp | Non-finite | Missing | Mean RGB Delta |")
    lines.append("|---|---:|---:|---:|---:|---:|---:|")
    for rep in density_reports:
        px = rep.get("pixels")
        rgb = f"{px['mean_rgb_delta']:.2f}" if px else "n/a"
        missing = len(rep["only_compose"]) + len(rep["only_cranpose"])
        pct = 100.0 * rep["within_tol_count"] / max(1, rep["common_count"])
        lines.append(
            f"| {rep['density_label']}x | {rep['common_count']} | {rep['exact_count']} | "
            f"{rep['within_tol_count']} ({pct:.1f}%) | {len(rep['non_finite_tags'])} | {missing} | {rgb} |"
        )

    lines.extend(["", "### Stress Suites", ""])
    for framework, srep in sorted(stress_reports.items()):
        scenarios = srep.get("scenarios", [])
        passed = sum(1 for s in scenarios if s["passed"])
        failed = [s["name"] for s in scenarios if not s["passed"]]
        suffix = f" (failing: {', '.join(failed)})" if failed else ""
        lines.append(f"- **{framework}**: {passed}/{len(scenarios)} passed{suffix}")

    if evaluation is not None:
        lines.extend(["", f"### Ledger Check: {'PASS' if evaluation['ok'] else 'FAIL'}", ""])
        for density, counts in sorted(evaluation["layout_counts"].items()):
            lines.append(
                f"- `{density}x`: {counts['expected']} expected divergences, "
                f"{counts['unexpected']} tracked unexpected bugs"
            )
        lines.append(f"- Tracked stress failures: {evaluation['tracked_stress_failures']}")
        if evaluation["errors"]:
            lines.extend(["", "#### Ledger Violations", ""])
            for err in evaluation["errors"]:
                lines.append(f"- {err}")
    return "\n".join(lines) + "\n"


def print_console_report(
    density_reports: list[dict[str, Any]],
    stress_reports: dict[str, dict[str, Any]],
    evaluation: dict[str, Any] | None,
) -> None:
    for rep in density_reports:
        pct = 100.0 * rep["within_tol_count"] / max(1, rep["common_count"])
        print("=" * 84)
        print(
            f"LAYOUT PARITY {rep['density_label']}x ({rep['width_dp']}x{rep['height_dp']} dp, "
            f"tol={rep['tol_dp']} dp): {rep['within_tol_count']}/{rep['common_count']} ({pct:.1f}%)"
        )
        print(
            f"  Exact={rep['exact_count']}  Non-finite={len(rep['non_finite_tags'])}  "
            f"OnlyCompose={len(rep['only_compose'])}  OnlyCranpose={len(rep['only_cranpose'])}"
        )
        for z in rep["zones"]:
            worst = "   inf" if z["max_delta"] is None else f"{z['max_delta']:6.2f}"
            status = "PASS" if z["mismatches"] == 0 else f"DIFF({z['mismatches']}/{z['total']})"
            print(f"  [{status:12s}] max={worst}dp  {z['zone']}")
    for framework, srep in sorted(stress_reports.items()):
        scenarios = srep.get("scenarios", [])
        passed = sum(1 for s in scenarios if s["passed"])
        print(f"STRESS [{framework}]: {passed}/{len(scenarios)} passed")
    if evaluation is not None:
        print(f"LEDGER: {'PASS' if evaluation['ok'] else 'FAIL'}")
        for err in evaluation["errors"]:
            print(f"  ERROR: {err}", file=sys.stderr)


def load_stress_reports(out_dir: Path) -> dict[str, dict[str, Any]]:
    reports: dict[str, dict[str, Any]] = {}
    for framework in ("compose", "cranpose"):
        path = out_dir / f"{framework}_stress.json"
        if path.exists():
            reports[framework] = json.loads(path.read_text(encoding="utf-8"))
    return reports


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=ROOT / "out", help="Output directory for reports and PNGs")
    parser.add_argument("--ledger", type=Path, default=DEFAULT_LEDGER, help="Path to divergences.toml")
    parser.add_argument("--run", action="store_true", help="Build and run Compose Desktop and Cranpose first")
    parser.add_argument("--check-ledger", action="store_true", help="Enforce divergences.toml ratchet")
    parser.add_argument("--densities", type=str, default=None, help="Comma-separated densities (default from ledger)")
    parser.add_argument("--cranpose-profile", choices=("debug", "release"), default="debug")
    parser.add_argument("--skip-images", action="store_true", help="Skip Pillow pixel diff and PNG outputs")
    parser.add_argument("--summary-md", type=Path, default=None, help="Optional path for markdown summary")
    args = parser.parse_args(argv)

    ledger = load_ledger(args.ledger)
    suite = ledger["suite"]
    width_dp = int(suite["width_dp"])
    height_dp = int(suite["height_dp"])
    tol_dp = float(suite["tol_dp"])
    densities = [s.strip() for s in args.densities.split(",")] if args.densities else [str(d) for d in suite["densities"]]

    if args.run:
        build_and_run_apps(args.out, width_dp, height_dp, densities, args.cranpose_profile)

    density_reports: list[dict[str, Any]] = []
    for label in densities:
        d_dir = args.out / density_dir_name(label)
        rep = compare_density_bounds(label, d_dir, tol_dp)
        if not args.skip_images:
            px = compare_images(d_dir)
            if px is not None:
                rep["pixels"] = px
        density_reports.append(rep)

    stress_reports = load_stress_reports(args.out)
    evaluation = evaluate_ledger(ledger, density_reports, stress_reports) if args.check_ledger else None
    print_console_report(density_reports, stress_reports, evaluation)

    summary_md = render_markdown_summary(density_reports, stress_reports, evaluation)
    summary_path = args.summary_md or (args.out / "summary.md")
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(summary_md, encoding="utf-8")

    serializable = {
        "densities": [{k: v for k, v in rep.items() if k != "entries"} for rep in density_reports],
        "stress": stress_reports,
        "ledger": evaluation,
    }
    (args.out / "parity_report.json").write_text(json.dumps(serializable, indent=2) + "\n", encoding="utf-8")
    return 0 if (evaluation is None or evaluation["ok"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
