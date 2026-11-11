# What this repository is

herdr-common holds what the apps on Herdr share for visual testing: herdr-bowtie,
herdr-progressive-reviewer, herdr-ruminate and enough-internet-for-today. Its Rust crates live
in `crates/<name>/`, which the workspace's `members = ["crates/*"]` takes as they land: a new
crate never touches the root `Cargo.toml`. `flake.nix` pins herdr, herdr-app and hc once for
all of them ([The pinned Herdr tools](.agents/wiki/herdr-tools.md)).

## How the apps use it

No crate here is published. An app takes a crate as a `git` dependency on this repository, its
`Cargo.lock` pinning the revision, or by `path` (herdr-common checked out beside the app) only
from a test workspace of its own, as herdr-ruminate's `tests/tui`: cargo reads every path
dependency of a workspace, dev-dependencies included. The private herdr-bowtie, always beside
herdr-common, may take plain path dependencies (ADR 0004).

The crates here are test tooling, never in what an app builds and installs (ADR 0001). The one
exception is `crates/vision-signal`, the frame signal an app links: it has no dependency, and
apps take it as a `git` dependency (ADR 0003). Any other code an app links stays out of
herdr-common, in a tiny crate of its own the app takes by git.

# Who owns the code

You are the owner and long-term maintainer of this codebase, and its design is up to you,
within the decision records (see Decisions). Agents build each change here unattended, and a
lead agent decides when it goes to main. As its owner, consolidate code duplicated within this
workspace into one shared place, and fix what you find broken around your change in the same
change. Name every change in the change description, the small ones too, and list under what
changed for the user each behaviour you changed that an app or the user sees. The lead reads
the diff beside the description, since an owner's description tends to leave small changes
out.

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

A change to the `herdr-mod-vision` skill or the `app-sandbox` command is tested by following the
skill on a real mod in the sandbox: see [The herdr-mod-vision skill](.agents/wiki/mod-vision.md).

A change to `flake.nix` is checked by `nix flake check`, which runs each pinned tool and
fails unless it reports its pinned version, and by the dev shell's variables: see
[The pinned Herdr tools](.agents/wiki/herdr-tools.md).

A change to docs alone is tested by searching README.md, AGENTS.md, CONTEXT.md,
CODING_STANDARDS.md, the ADRs, the wiki, the skills and the crates' doc comments for each
statement of what it changes, until all agree with it.

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

A test that needs herdr-app runs it in `crates/app-sandbox`; read
[The app sandbox](.agents/wiki/app-sandbox.md) before you change or test that sandbox.

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
