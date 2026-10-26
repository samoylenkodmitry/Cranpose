# Rule evals

Each case checks whether the code rules of `AGENTS.md` change what an agent
writes, the method of
[BeautifulRust](https://github.com/morganlinton/BeautifulRust/tree/main/skills/rust-best-practices/evals):
a task prompt, an expected outcome, and graders that an LLM judge scores.
`just rule-evals` turns the rules into a skill (`make_skill.py`) and runs every
case twice, with the skill and without it, through `claude plugin eval`; the
report gives each grader's score in both arms and the difference.

The cases test rules no lint checks:

- `flag-and-id-types`: ids as newtypes, enums for arguments that select
  behavior, `Option` for absence.
- `borrow-without-clone`: no clone made for the borrow checker.
- `public-entry-tests`: tests through public entry points, at the corner cases.

Run a case after changing the rule it covers, and add a case for a new rule
before adding the rule. A rule that does not move its graders costs tokens in
every session and buys nothing.
