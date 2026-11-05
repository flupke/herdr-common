# Coding standards

The rules every change to herdr-common follows, in its code, tests, docs and the text its agents
read. Code review checks the change against each rule, and a finding names the rule it breaks.
Leave out what the checks already enforce: clippy with `-Dwarnings`, and the complexity gate of
`make lint`.

Keep this file under 100 lines. When a topic outgrows it, move the topic to a page of
`.agents/wiki/` and leave a line here that says when to read the page.

## Names

- Name things with the terms of `CONTEXT.md`, in code, docs and UI text, and use none of the
  synonyms it lists to avoid.

## Design

- herdr-common is open source: what it runs assumes none of the maintainer's own tools or paths
  (a dotfiles manager, a login shell, a checkout location). It does assume Nix: the tools it
  runs come from the dev shell.
- A crate here is test tooling: no app links it into what it builds and installs (AGENTS.md,
  How the apps use it).
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

- `.agents/wiki/unit-tests.md` holds the testing policy and the kinds of tautological test:
  read it before you review a test, and hold each test the change adds or keeps to it.

## Docs

- `README.md` is for people deciding whether to use herdr-common: what it holds, how an app
  uses it. History goes in change descriptions, decisions in `.agents/adr/`, knowledge for
  agents in the wiki, and how to develop here in `AGENTS.md`.

## Text for agents

- Text that agents read follows the `writing-for-agents` skill: `AGENTS.md`, this file, the
  wiki and the skills. Load the skill before you write or review such text.
