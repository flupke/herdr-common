# Coding standards

The rules every change to herdr-common follows, in its code, tests, docs and the text its agents
read. Code review checks the change against each rule, and a finding names the rule it breaks.
Leave out what the checks already enforce: clippy with `-Dwarnings`, and the complexity gate of
`make lint`.

Keep this file under 100 lines.

## Names

- Name things with the terms of `CONTEXT.md`, in code, docs and UI text, and use none of the
  synonyms it lists to avoid.

## Design

- herdr-common is open source: what it runs assumes none of the user's own tools or paths
  (a dotfiles manager, a login shell, a checkout location). It does assume Nix: the tools it
  runs come from the dev shell.
- A crate here is test tooling, which no app links into what it builds and installs (AGENTS.md,
  How the apps use it), save `vision-signal`.
- A crate serves every app that needs it: what is particular to one app stays in that app,
  behind a parameter or a trait the crate takes.
- A test reaches Herdr, herdr-app and hc only through the pinned releases the dev shell names
  (`TEST_*_BIN_PATH`), never through `PATH`, where the user's own stay.

## Rust

- Keep crates small: a tiny crate is welcome, and a big one is worth splitting.
- Reuse existing types between workspace crates. Do not create a local copy of a type only to
  adapt it for another crate.
- Use `#[must_use]` only when ignoring a return value is likely to cause a bug. Do not add it to
  routine getters or to functions that return types that already have this attribute.
- Use the smallest visibility that permits the required use. The `unreachable_pub` lint catches
  a needless `pub`; review catches a `pub(crate)` that only its own module uses.
- A crate takes the workspace's lints and package fields: `[lints] workspace = true`, and
  `version`, `edition`, `rust-version` and `license` as `.workspace = true`.

## Tests

- Test the behaviour that matters once, at the lowest level that sees it: what an app relies
  on, such as a sandbox that starts the pinned tool and stops all it started. No tests for
  crash, restart or race recovery unless the user asks. Every change deletes the tests it makes
  obsolete or redundant: a smaller suite counts as progress.
- A test earns its place by two edits: break the behaviour (a wrong value, an effect skipped or
  done twice) and it fails; change only the internals, every observable result the same, and it
  still passes. Rewrite or delete a test that fails either.
- A crate made from the apps' code gets its own test of each behaviour the apps' tests rely on.
- A tool an agent drives an app with is tested as the agent uses it: through an MCP client, on
  a small app in the app sandbox whose behaviour makes a wrong tool fail.
- Write the expected value by hand from the behaviour owed, or take it from an independent
  source; compare a format this repository owns with a literal.
- A fix for slowness is tested by the work the code does, never by a time bound: count what the
  slow path multiplies. Run the test on the old code once, to see it go red.
- Delete, rewrite, or mark ignored a tautological test:
  - **Mirror**: the expected value comes from the code under test, or a copy of its logic.
  - **Echo**: it reads back a field it set, a constructor argument, or a clone.
  - **Constant pin**: `assert_eq!(LIMIT, 16_777_216)`; build the boundary input from it instead.
  - **Library behaviour**: std, plain serde derives, a dependency's own behaviour.
  - **Fake echo**: every value asserted comes from a fake, through code that only passes it on.
  - **Vacuous assertion**: true for every value, or met by an error as well as a success.
  - **Harness counted as a test**: a child-process `#[test]` that returns early unless a
    variable is set: mark it `#[ignore = "<what runs it>"]`, and the parent passes `--ignored`.
- A test waits on events, never on the clock: give the code its durations or its clock as
  values, wake the test with an event (a channel, Herdr's event stream, `hc events`), and prove
  that nothing happened by sending a marker through the same path. nextest ends a test stuck
  for two minutes. A screen sends no event: waiting for a terminal's text or the window's pixels
  to settle is the only poll a test may run.

## Docs

- `README.md` is for people deciding whether to use herdr-common: what it holds, how an app
  uses it. History goes in change descriptions, a reason that code alone cannot show in a
  comment at that code, how to develop here in `AGENTS.md`, and the procedures of a tool, with
  what Herdr, herdr-app or hc do, in a skill.

## Text for agents

- Text that agents read follows the `writing-for-agents` skill: `AGENTS.md`, this file and
  the skills. Load the skill before you write or review such text.
