"""Unit tests for benchmarks/layout-parity/compare.py and divergences.toml."""

from __future__ import annotations

import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

BENCH_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCH_ROOT))

import compare  # noqa: E402


def write_bounds(path: Path, framework: str, nodes: dict[str, dict[str, float | str]]) -> None:
    payload = {
        "framework": framework,
        "width_dp": 1760,
        "height_dp": 980,
        "density": 1.0,
        "nodes": nodes,
    }
    path.write_text(json.dumps(payload), encoding="utf-8")


def sample_report(
    compose_nodes: dict[str, dict[str, float | str]],
    cranpose_nodes: dict[str, dict[str, float | str]],
    density: str = "1.0",
) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        write_bounds(d / "compose_bounds.json", "compose", compose_nodes)
        write_bounds(d / "cranpose_bounds.json", "cranpose", cranpose_nodes)
        return compare.compare_density_bounds(density, d, tol_dp=0.5)


def rect(x: float, y: float, w: float, h: float) -> dict[str, float]:
    return {"x": x, "y": y, "width": w, "height": h}


class CompareLedgerTests(unittest.TestCase):
    def test_matching_expected_and_unexpected_entries_pass(self) -> None:
        rep = sample_report(
            {
                "z1_ok": rect(0, 0, 10, 10),
                "z6_text": rect(0, 0, 50, 14),
                "z11_off": rect(10, 10, 20, 20),
                "z17_inf": rect(0, 0, 100, 30),
                "z17_pushed": rect(0, 40, 100, 30),
            },
            {
                "z1_ok": rect(0, 0, 10, 10),
                "z6_text": rect(0, 0, 50, 15.2),
                "z11_off": rect(4, 10, 20, 20),
                "z17_inf": {"x": 0.0, "y": 0.0, "width": 100.0, "height": "Infinity"},
            },
        )
        ledger = {
            "layout": {
                "expected": [
                    {"id": "text-lh", "densities": ["1.0"], "kind": "bounds_delta", "max_dp": 2.0, "tags": ["z6_text"]}
                ],
                "unexpected": [
                    {
                        "id": "offset-bounds",
                        "issue": "#2459",
                        "densities": ["1.0"],
                        "kind": "bounds_delta",
                        "max_dp": 6.5,
                        "tags": ["z11_off"],
                    },
                    {
                        "id": "lazy-inf",
                        "issue": "#2458",
                        "densities": ["1.0"],
                        "kind": "non_finite",
                        "tags": ["z17_inf"],
                    },
                    {
                        "id": "lazy-missing",
                        "issue": "#2458",
                        "densities": ["1.0"],
                        "kind": "missing_in_cranpose",
                        "tags": ["z17_pushed"],
                    },
                ],
            },
            "stress": {
                "unexpected": [
                    {"framework": "cranpose", "scenario": "owner_disposed", "issue": "#2457"},
                ]
            },
        }
        stress = {
            "compose": {"scenarios": [{"name": "ok_case", "passed": True, "panic": None}]},
            "cranpose": {
                "scenarios": [
                    {"name": "ok_case", "passed": True, "panic": None},
                    {"name": "owner_disposed", "passed": False, "panic": "boom"},
                ]
            },
        }
        ev = compare.evaluate_ledger(ledger, [rep], stress)
        self.assertTrue(ev["ok"], ev["errors"])
        self.assertEqual(ev["layout_counts"]["1.0"], {"expected": 1, "unexpected": 3})
        self.assertEqual(ev["tracked_stress_failures"], 1)

    def test_unlisted_regression_and_worsened_delta_fail(self) -> None:
        rep = sample_report(
            {"z1_new": rect(0, 0, 10, 10), "z11_off": rect(10, 10, 20, 20)},
            {"z1_new": rect(0, 0, 12, 10), "z11_off": rect(0, 10, 20, 20)},
        )
        ledger = {
            "layout": {
                "expected": [],
                "unexpected": [
                    {
                        "id": "offset-bounds",
                        "issue": "#2459",
                        "densities": ["1.0"],
                        "kind": "bounds_delta",
                        "max_dp": 6.5,
                        "tags": ["z11_off"],
                    }
                ],
            },
            "stress": {"unexpected": []},
        }
        ev = compare.evaluate_ledger(ledger, [rep], {})
        self.assertFalse(ev["ok"])
        joined = "\n".join(ev["errors"])
        self.assertIn("unlisted layout divergence: 'z1_new'", joined)
        self.assertIn("regression on 'z11_off'", joined)

    def test_fixed_unexpected_layout_and_stress_entries_fail_ratchet(self) -> None:
        rep = sample_report(
            {"z11_off": rect(10, 10, 20, 20), "z6_text": rect(0, 0, 50, 14)},
            {"z11_off": rect(10, 10, 20, 20), "z6_text": rect(0, 0, 50, 14)},
        )
        ledger = {
            "layout": {
                "expected": [
                    {"id": "text-lh", "densities": ["1.0"], "kind": "bounds_delta", "max_dp": 2.0, "tags": ["z6_text"]}
                ],
                "unexpected": [
                    {
                        "id": "offset-bounds",
                        "issue": "#2459",
                        "densities": ["1.0"],
                        "kind": "bounds_delta",
                        "max_dp": 6.5,
                        "tags": ["z11_off"],
                    }
                ],
            },
            "stress": {
                "unexpected": [{"framework": "cranpose", "scenario": "owner_disposed", "issue": "#2457"}]
            },
        }
        stress = {
            "cranpose": {"scenarios": [{"name": "owner_disposed", "passed": True, "panic": None}]}
        }
        ev = compare.evaluate_ledger(ledger, [rep], stress)
        self.assertFalse(ev["ok"])
        joined = "\n".join(ev["errors"])
        self.assertIn("fixed unexpected bug: 'z11_off'", joined)
        self.assertIn("fixed unexpected panic: 'owner_disposed'", joined)
        self.assertNotIn("z6_text", joined)

    def test_unlisted_stress_failure_and_duplicate_rules_fail(self) -> None:
        rep = sample_report({"z1_a": rect(0, 0, 10, 10)}, {"z1_a": rect(0, 0, 10, 10)})
        ledger = {
            "layout": {
                "expected": [
                    {"id": "r1", "densities": ["1.0"], "kind": "bounds_delta", "max_dp": 1.0, "tags": ["z1_a"]},
                    {"id": "r2", "densities": ["1.0"], "kind": "bounds_delta", "max_dp": 1.0, "tags": ["z1_a"]},
                ],
                "unexpected": [],
            },
            "stress": {"unexpected": []},
        }
        stress = {"cranpose": {"scenarios": [{"name": "new_crash", "passed": False, "panic": "oops"}]}}
        ev = compare.evaluate_ledger(ledger, [rep], stress)
        self.assertFalse(ev["ok"])
        joined = "\n".join(ev["errors"])
        self.assertIn("duplicate ledger entry", joined)
        self.assertIn("unlisted stress failure: 'new_crash'", joined)

    def test_checked_in_ledger_is_well_formed(self) -> None:
        ledger = compare.load_ledger(BENCH_ROOT / "divergences.toml")
        _, errors = compare.index_layout_rules(ledger)
        self.assertEqual(errors, [])
        issue_re = re.compile(r"^#\d+$")
        for rule in ledger.get("layout", {}).get("unexpected", []):
            self.assertRegex(rule["issue"], issue_re)
            self.assertTrue(rule["tags"], f"empty tags in {rule['id']}")
        for rule in ledger.get("stress", {}).get("unexpected", []):
            self.assertRegex(rule["issue"], issue_re)


if __name__ == "__main__":
    unittest.main()
