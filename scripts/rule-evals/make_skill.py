#!/usr/bin/env python3
"""Writes skills/cranpose-rules/SKILL.md from the code and performance rules
of AGENTS.md, so the rules the evals measure stay written once.

usage: make_skill.py
"""
import pathlib

here = pathlib.Path(__file__).resolve().parent
agents = (here.parent.parent / "AGENTS.md").read_text()
start = agents.index("Code rules (")
end = agents.index("Read the matching workflow")
skill = here / "skills" / "cranpose-rules" / "SKILL.md"
skill.parent.mkdir(parents=True, exist_ok=True)
skill.write_text(
    "---\n"
    "name: cranpose-rules\n"
    "description: >-\n"
    "  The code rules of the Cranpose workspace. Use whenever you write, refactor or\n"
    "  test Rust code here.\n"
    "---\n\n" + agents[start:end].rstrip() + "\n"
)
print(f"wrote {skill.relative_to(here.parent.parent)}")
