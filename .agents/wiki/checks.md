# Checks

Gotchas of the check commands. [AGENTS.md](../../AGENTS.md) lists the check levels (step 2) and
says which gate a change runs (step 5); the `Makefile` says what each target runs. What neither
says:

- Cargo refuses a `crates/*` glob that matches no folder ("failed to load manifest for
  workspace member `…/crates/*`"), and a workspace with no member ("the manifest is virtual,
  and the workspace has no members"). So until the first crate lands in
  `crates/`, the Makefile skips its cargo steps (clippy, cccc, the tests, `cargo fmt`), and
  `make check` runs `typos` alone. `cargo` run by hand fails the same way meanwhile.
- nextest fails a run that finds no test: the Makefile passes `--no-tests=pass`, so a crate
  with no test yet passes.
- nextest runs no doc test, so `make test` with `CRATES` runs none: only `make test` without it
  runs the doc tests.
- `typos` skips hidden folders unless named, so `make lint` names `.agents`; a new hidden
  folder of text needs adding there. A word `typos` flags that is right goes in `_typos.toml`
  (not there yet), under `[default.extend-words]`.
- `.config/nextest.toml` ends a test still running after two minutes: the hang guard of tests
  that wait on events without a deadline ([Tests](unit-tests.md), Waiting).
