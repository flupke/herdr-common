# What this repository is

herdr-common holds what the apps on Herdr share for visual testing: herdr-bowtie,
herdr-progressive-reviewer and enough-internet-for-today. Its Rust crates live in
`crates/<name>/`, which the workspace's `members = ["crates/*"]` takes as they land: a new
crate never touches the root `Cargo.toml`. `flake.nix` pins herdr, herdr-app and hc once for
all of them ([The pinned Herdr tools](.agents/wiki/herdr-tools.md)).

## How the apps use it

An app takes a crate through a cargo `path` dependency, `../herdr-common/crates/<name>`: no
publishing, no git revision pins. So the crates here are test tooling only: an app takes them as
`[dev-dependencies]` or from a test workspace of its own (as the reviewer's `tests/tui`), never
in what it builds and installs. Code an app itself links, such as the reviewer's vision-signal
frame emitter, stays out of herdr-common, in a tiny crate the app takes as a `git` dependency.
ADR 0001 says why.

# Standards

Read `CODING_STANDARDS.md`, the coding standards, before you write a change.
While you work:

- When the user corrects a concept, fix its entry in `CONTEXT.md` in the same change.
- herdr-bowtie and herdr-progressive-reviewer came first, on the same stack, checked out
  beside this repository (`../herdr-bowtie`, `../herdr-progressive-reviewer`): read how they
  do a thing before you design it here. Most crates here start as code moved out of them.
- Design types first, then implement their methods.

# Tests

Before you write a test, read `.agents/wiki/unit-tests.md`: it says which tests earn their
place. Every change deletes the tests it makes obsolete or redundant. Acceptance criteria
name outcomes the user can see, not lists of test cases.

A change to `flake.nix` is checked by `nix flake check`, which runs each pinned tool and
fails unless it reports its pinned version, and by the dev shell's variables: see
[The pinned Herdr tools](.agents/wiki/herdr-tools.md).

# Wiki

The wiki is a playbook for working in this repository, one topic per page, listed in
`.agents/wiki/index.md`. Read the page for a tool (the checks, the pinned Herdr tools) before
you run or debug it. The workflow says when to read the pages of an area (step 1) and when to
update them (step 2).

A line earns its place when an agent working here needs it and cannot get it cheaply from the
code, the tests or a `--help`:

- how to run a tool, and its gotchas;
- the conclusions of expensive runs, and the commands that run them again;
- how an outside tool behaves, such as Herdr, herdr-app, jj, Nix and Claude Code;
- the reason for a choice, or a guarantee that spans several files, which no single file can
  state.

What a module does, its files and the behaviour a test checks stay in the code and the tests.
A page says how things work now: the account of a past run goes in the change description.
Give a new topic its own page, and add its line to the index. Knowledge for agents goes in the
wiki, and decisions in `.agents/adr/`.

# Decisions

`.agents/adr/` holds the decision records (ADRs), one decision per file. A record keeps a
decision from being undone by accident.

- When a change would contradict a record, tell the user which record, and why its reasons may
  no longer hold. Change the design once the user agrees.
- Record a decision when all three hold: it is hard to reverse, it would surprise a reader who
  lacks the context, and a real alternative was weighed. Say the situation, the choice, and the
  alternative given up with the reason.
- A record is frozen. When a decision changes, write a new record that names the one it
  supersedes, and leave the old text as it was.

# Change workflow

Follow these steps for every change: a feature, a fix, or a change to docs.

1. Create a fresh jj change before implementation. Use the previous change as the fixed point
   for this change. Read the wiki pages and the decision records of the area you will change.
2. Implement the change, and check it as you go with the levels that fit it, cheapest first:
   1. `make lint`: clippy on every target, the complexity gate, and spelling.
   2. `make test CRATES="crate-a crate-b"`: the unit tests of the crates the change touches.
   3. `make test`: every unit test of the workspace.
   Once the change is complete, run `make fmt` and bring the wiki up to date: edit each line
   the change made false, and add a line only when it earns its place (see Wiki). Record a
   decision that the change made, when the decision passes the test in Decisions.
3. For a change the user can see, run it as its user would, look at it, and fix what you find.
4. Invoke the `code-review` skill against the fixed point; the subagents it starts are
   authorized. Check each finding against the code before acting on it, fix those that hold,
   and review again until a round finds nothing that you fix, for at most two rounds. Report
   any finding you leave unfixed, with the reason.
5. Once step 4 is over, run the gate, with the change as the working copy: `make check`, and
   `nix flake check` when the change touches `flake.nix` or `flake.lock`. Fix whatever fails,
   even outside the change, run `make fmt`, and run the gate again until it passes. This
   repository runs no CI: before you push for a pull request, `jj git fetch`, rebase the change
   onto `main` and run the gate there.
6. Check the wiki against the change once more. Then describe the change with `jj describe`: a
   plain imperative subject, then a first paragraph on why, then what changed for the user.
7. Keep later user-feedback fixes in the same change, and run the gate again once each set of
   them is done. Create another change only when the user asks for something new.

Run the checks in the dev shell, through `scripts/check`: `scripts/dev-shell scripts/check
check`. The dev shell brings the pinned Rust toolchain, the tools the checks run and the
pinned Herdr tools, in the default workspace and in any other jj workspace alike; `scripts/check`
keeps the whole output under `target/checks/` and ends on the check's status. Keep the warning
gate of the checks (`-Dwarnings`) enabled; fix warnings in the current change instead of
overriding it.

When you work in a jj workspace, read `.agents/wiki/parallel-workspaces.md`: how to enter the
dev shell there, and clean up.

## When herdr-bowtie runs you

herdr-bowtie runs its workers on this repository too, each in its task's jj workspace. There,
its procedures replace steps 1, 4, 6 and 7: start, review, describe and ship as they say. Steps
2, 3 and 5 still apply.

# Sandbox tests

A test that needs Herdr or herdr-app runs the pinned release named in the dev shell's
`TEST_*_BIN_PATH` variables, with its own socket, config, state and display. Never use,
restart or modify the user's Herdr, herdr-app or their panes during tests. Stop only the
processes you started, by their PID: other agents and the user's sessions run on this machine.

# Skill settings

## Agent skills

### Issue tracker

Issues live in GitHub Issues for flupke/herdr-common (via `gh`). See
`.agents/settings/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`,
`ready-for-human`, `wontfix`). See `.agents/settings/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` plus `.agents/adr/`. See `.agents/settings/domain.md`.
