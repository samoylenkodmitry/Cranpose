#!/usr/bin/env python3

from __future__ import annotations

import argparse
import math
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageStat


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--trim-top", type=int, required=True)
    parser.add_argument("--trim-bottom", type=int, required=True)
    parser.add_argument("--trim-left", type=int, default=0)
    parser.add_argument("--trim-right", type=int, default=0)
    parser.add_argument("--max-offset", type=int, required=True)
    parser.add_argument("--max-adjacent-score", type=int, default=0)
    parser.add_argument("--stabilized-guard", type=int, default=0)
    parser.add_argument("images", nargs="+")
    return parser.parse_args()


def overlap_boxes(height: int, trim_top: int, trim_bottom: int, dy: int) -> tuple[tuple[int, int], tuple[int, int]]:
    if dy >= 0:
        ref_top = trim_top + dy
        ref_bottom = trim_bottom
        cand_top = trim_top
        cand_bottom = trim_bottom + dy
    else:
        shift = -dy
        ref_top = trim_top
        ref_bottom = trim_bottom + shift
        cand_top = trim_top + shift
        cand_bottom = trim_bottom

    ref_height = height - ref_top - ref_bottom
    cand_height = height - cand_top - cand_bottom
    if ref_height <= 0 or cand_height <= 0 or ref_height != cand_height:
        raise ValueError(
            f"invalid overlap for dy={dy}: ref=({ref_top},{ref_bottom}) cand=({cand_top},{cand_bottom})"
        )
    return (ref_top, ref_bottom), (cand_top, cand_bottom)


def crop_viewport(image: Image.Image, top: int, bottom: int, left: int, right: int) -> Image.Image:
    width, height = image.size
    crop_width = width - left - right
    crop_height = height - top - bottom
    if left < 0 or right < 0 or top < 0 or bottom < 0 or crop_width <= 0 or crop_height <= 0:
        raise ValueError(
            f"invalid crop: left={left} right={right} top={top} bottom={bottom} size={image.size}"
        )
    return image.crop((left, top, width - right, height - bottom))


def diff_image_path(reference_path: Path, step_path: Path) -> Path:
    return reference_path.parent / f"diff_{reference_path.stem[-2:]}_{step_path.stem[-2:]}.png"


def stabilized_step_path(image_path: Path) -> Path:
    return image_path.parent / f"stabilized_{image_path.stem}.png"


def fractional_stabilized_step_path(image_path: Path) -> Path:
    return image_path.parent / f"fractional_stabilized_{image_path.stem}.png"


def fractional_report_path(output_dir: Path) -> Path:
    return output_dir / "fractional_alignment_report.txt"


def fractional_diff_path(reference_path: Path, step_path: Path) -> Path:
    return reference_path.parent / f"fractional_diff_{reference_path.stem[-2:]}_{step_path.stem[-2:]}.png"


def cleanup_derived_outputs(output_dir: Path) -> None:
    for pattern in (
        "diff_*.png",
        "trim_*.png",
        "stabilized_*.png",
        "fractional_stabilized_*.png",
        "fractional_aligned_*.png",
        "fractional_diff_*.png",
        "fractional_alignment_report.txt",
    ):
        for path in output_dir.glob(pattern):
            path.unlink(missing_ok=True)


def save_visual_diff(reference: Image.Image, candidate: Image.Image, output_path: Path) -> None:
    diff = ImageChops.difference(reference.convert("RGB"), candidate.convert("RGB"))
    boosted = diff.point(lambda p: min(255, p * 16))
    boosted.save(output_path)


def score_difference(reference: Image.Image, candidate: Image.Image) -> tuple[int, tuple[int, int, tuple[int, int, int, int], tuple[int, int, int, int]] | None]:
    if reference.size != candidate.size:
        raise ValueError(f"size mismatch: {reference.size} vs {candidate.size}")

    if reference.tobytes() == candidate.tobytes():
        return 0, None

    diff = ImageChops.difference(reference, candidate)
    stat = ImageStat.Stat(diff)
    score = int(sum(stat.sum))

    ref_px = reference.load()
    cand_px = candidate.load()
    width, height = reference.size
    for y in range(height):
        for x in range(width):
            before = ref_px[x, y]
            after = cand_px[x, y]
            if before != after:
                return score, (x, y, before, after)

    return score, None


def search_best_alignment(
    reference: Image.Image,
    candidate: Image.Image,
    trim_top: int,
    trim_bottom: int,
    trim_left: int,
    trim_right: int,
    max_offset: int,
) -> tuple[
    int,
    int,
    tuple[int, int],
    tuple[int, int],
    Image.Image,
    Image.Image,
    tuple[int, int, tuple[int, int, int, int], tuple[int, int, int, int]] | None,
]:
    width, height = reference.size
    if candidate.size != (width, height):
        raise ValueError(f"size mismatch: {reference.size} vs {candidate.size}")

    best = None
    for dy in range(-max_offset, max_offset + 1):
        ref_box, cand_box = overlap_boxes(height, trim_top, trim_bottom, dy)
        ref_crop = crop_viewport(reference, ref_box[0], ref_box[1], trim_left, trim_right)
        cand_crop = crop_viewport(candidate, cand_box[0], cand_box[1], trim_left, trim_right)
        score, first_diff = score_difference(ref_crop, cand_crop)
        if best is None or score < best[0]:
            best = (score, dy, ref_box, cand_box, ref_crop, cand_crop, first_diff)
        if score == 0:
            break

    assert best is not None
    return best


def crop_with_guard(image: Image.Image, guard: int) -> Image.Image:
    width, height = image.size
    guarded_height = height - guard * 2
    if guarded_height <= 0:
        raise ValueError(f"invalid guard {guard} for image height {height}")
    return image.crop((0, guard, width, height - guard))


def shift_vertical(image: Image.Image, shift_down_px: float) -> Image.Image:
    return image.transform(
        image.size,
        Image.Transform.AFFINE,
        (1.0, 0.0, 0.0, 0.0, 1.0, -shift_down_px),
        resample=Image.Resampling.BILINEAR,
    )


def fractional_shift_score(reference: Image.Image, candidate: Image.Image, shift_down_px: float) -> int:
    reference_crop, shifted_crop = fractional_aligned_pair(reference, candidate, shift_down_px)
    score, _ = score_difference(reference_crop, shifted_crop)
    return score


def search_best_fractional_shift(reference: Image.Image, candidate: Image.Image) -> tuple[float, int]:
    coarse_best = None
    for coarse_step in range(-10, 11):
        shift = coarse_step / 10.0
        score = fractional_shift_score(reference, candidate, shift)
        if coarse_best is None or score < coarse_best[0]:
            coarse_best = (score, shift)

    assert coarse_best is not None
    _, coarse_shift = coarse_best

    fine_best = None
    for fine_step in range(-10, 11):
        shift = coarse_shift + fine_step / 100.0
        score = fractional_shift_score(reference, candidate, shift)
        if fine_best is None or score < fine_best[1]:
            fine_best = (shift, score)

    assert fine_best is not None
    return fine_best


def fractional_aligned_pair(
    reference: Image.Image,
    candidate: Image.Image,
    shift_down_px: float,
) -> tuple[Image.Image, Image.Image]:
    shifted = shift_vertical(candidate, shift_down_px)
    guard = 2 + math.ceil(abs(shift_down_px))
    reference_crop = crop_with_guard(reference, guard)
    shifted_crop = crop_with_guard(shifted, guard)
    return reference_crop, shifted_crop


def crop_with_explicit_guard(image: Image.Image, guard: int) -> Image.Image:
    return crop_with_guard(image, guard)


def anchored_trim_pair(
    height: int,
    base_trim_top: int,
    base_trim_bottom: int,
    dy_from_reference: int,
) -> tuple[int, int]:
    trim_top = base_trim_top - dy_from_reference
    trim_bottom = base_trim_bottom + dy_from_reference
    visible_height = height - trim_top - trim_bottom
    if trim_top < 0 or trim_bottom < 0 or visible_height <= 0:
        raise ValueError(
            f"invalid stabilized trim for shift={dy_from_reference}: "
            f"top={trim_top} bottom={trim_bottom} height={height}"
        )
    return trim_top, trim_bottom


def main() -> int:
    args = parse_args()
    image_paths = [Path(path) for path in args.images]
    images = [Image.open(path).convert("RGBA") for path in image_paths]

    if len(images) < 2:
        raise SystemExit("need at least two screenshots")

    width, height = images[0].size
    for path, image in zip(image_paths, images):
        if image.size != (width, height):
            raise SystemExit(f"{path}: expected size {(width, height)}, got {image.size}")

    cleanup_derived_outputs(image_paths[0].parent)

    had_failure = False
    reference = images[0]
    stabilized_reference = crop_viewport(
        reference,
        args.trim_top,
        args.trim_bottom,
        args.trim_left,
        args.trim_right,
    )
    stabilized_reference.save(stabilized_step_path(image_paths[0]))
    stabilized_images = [stabilized_reference]
    anchor_dys = [0]

    for step in range(1, len(images)):
        _, dy, _, _, _, _, _ = search_best_alignment(
            reference,
            images[step],
            args.trim_top,
            args.trim_bottom,
            args.trim_left,
            args.trim_right,
            args.max_offset,
        )
        trim_top, trim_bottom = anchored_trim_pair(
            height,
            args.trim_top,
            args.trim_bottom,
            dy,
        )
        stabilized_current = crop_viewport(
            images[step],
            trim_top,
            trim_bottom,
            args.trim_left,
            args.trim_right,
        )
        stabilized_current.save(stabilized_step_path(image_paths[step]))
        stabilized_images.append(stabilized_current)
        anchor_dys.append(dy)

    report_lines = [
        "fractional_alignment_report",
        "strict_contract=exact_pixel_match_on_adjacent_stabilized_frames",
        "fractional_diagnostic=apply_bilinear_vertical_shift_to_step00_anchored_stabilized_frames_for_measurement_only",
        "fractional_shift_direction=positive_moves_current_frame_down",
        "guard_px=2+ceil(abs(fractional_shift_px))",
        "",
    ]

    exact_adjacent_scores = [0]
    for step in range(1, len(stabilized_images)):
        stabilized_reference = crop_with_explicit_guard(
            stabilized_images[step - 1],
            args.stabilized_guard,
        )
        stabilized_current = crop_with_explicit_guard(
            stabilized_images[step],
            args.stabilized_guard,
        )
        score, first_diff = score_difference(stabilized_reference, stabilized_current)
        exact_adjacent_scores.append(score)
        if score == 0:
            print(
                f"step {step - 1:02}->{step:02}: exact adjacent stabilized match"
            )
            continue
        if score <= args.max_adjacent_score:
            print(
                f"step {step - 1:02}->{step:02}: adjacent stabilized match "
                f"within score budget score={score} budget={args.max_adjacent_score}"
            )
            continue

        had_failure = True
        save_visual_diff(
            stabilized_reference,
            stabilized_current,
            diff_image_path(image_paths[step - 1], image_paths[step]),
        )
        if first_diff is None:
            print(
                f"step {step - 1:02}->{step:02}: adjacent stabilized mismatch "
                f"score={score} with no first diff located"
            )
            continue

        x, y, before, after = first_diff
        print(
            f"step {step - 1:02}->{step:02}: adjacent stabilized mismatch "
            f"score={score} first_diff=({x},{y}) reference={before} current={after}"
        )

    fractional_anchor_shifts = [0.0]
    fractional_anchor_scores = [0]
    for step in range(1, len(stabilized_images)):
        shift_px, anchor_score = search_best_fractional_shift(
            stabilized_images[0],
            stabilized_images[step],
        )
        fractional_anchor_shifts.append(shift_px)
        fractional_anchor_scores.append(anchor_score)

    max_fractional_guard = 2 + max(math.ceil(abs(shift_px)) for shift_px in fractional_anchor_shifts)
    fractional_stabilized_images = []
    for step, image in enumerate(stabilized_images):
        shifted = shift_vertical(image, fractional_anchor_shifts[step])
        fractional_stabilized = crop_with_explicit_guard(shifted, max_fractional_guard)
        fractional_stabilized.save(fractional_stabilized_step_path(image_paths[step]))
        fractional_stabilized_images.append(fractional_stabilized)

    report_lines.append("fractional_anchor_to_step00")
    for step in range(len(stabilized_images)):
        report_lines.append(
            " ".join(
                [
                    f"step=00->{step:02}",
                    f"anchor_dy={anchor_dys[step]}",
                    f"fractional_anchor_shift_px={fractional_anchor_shifts[step]:.2f}",
                    f"fractional_anchor_score={fractional_anchor_scores[step]}",
                ]
            )
        )

    report_lines.append("")
    report_lines.append("adjacent_comparison")
    for step in range(1, len(fractional_stabilized_images)):
        fractional_reference = fractional_stabilized_images[step - 1]
        fractional_current = fractional_stabilized_images[step]
        fractional_score, _ = score_difference(fractional_reference, fractional_current)
        save_visual_diff(
            fractional_reference,
            fractional_current,
            fractional_diff_path(image_paths[step - 1], image_paths[step]),
        )
        report_lines.append(
            " ".join(
                [
                    f"step={step - 1:02}->{step:02}",
                    f"exact_score={exact_adjacent_scores[step]}",
                    f"fractional_score={fractional_score}",
                    f"improvement={exact_adjacent_scores[step] - fractional_score}",
                ]
            )
        )

    fractional_report_path(image_paths[0].parent).write_text("\n".join(report_lines) + "\n", encoding="utf-8")

    return 1 if had_failure else 0


if __name__ == "__main__":
    sys.exit(main())
