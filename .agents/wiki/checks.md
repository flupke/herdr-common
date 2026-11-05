# Checks

Gotchas of the check commands. [AGENTS.md](../../AGENTS.md) lists the check levels (step 2) and
says which gate a change runs (step 5); the `Makefile` says what each target runs. What neither
says:

- nextest fails a run that finds no test: the Makefile passes `--no-tests=pass`, so a crate
  with no test yet passes.
- nextest runs no doc test, so `make test` with `CRATES` runs none: only `make test` without it
  runs the doc tests.
- `typos` skips hidden folders unless named, so `make lint` names `.agents`; a new hidden
  folder of text needs adding there. A word `typos` flags that is right goes in `_typos.toml`
  (not there yet), under `[default.extend-words]`.
- `.config/nextest.toml` ends a test still running after two minutes: the hang guard of tests
  that wait on events without a deadline ([Tests](unit-tests.md), Waiting).
