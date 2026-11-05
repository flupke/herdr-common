# Each app runs the fake agent from a program of its own

herdr-bowtie and herdr-progressive-reviewer each had a fake Claude Code for their tests. The
bowtie's ran from its e2e tests, as an ignored test a launcher script started; the
reviewer's was a binary of its own crate, which its tests found through `CARGO_BIN_EXE_`. Merged
into `crates/fake-agent`, the agent must reach both apps' panes, and it differs per app: the
subcommand that runs the app's hooks, and what a prompt reads (the reviewer reads a turn's files
with it).

`fake-agent` is a library: an app calls `fake_agent::run(&App, args)` from a program of its own,
with an `App` value that holds what differs per app. What differs per test reaches the agent in
`FAKE_AGENT_*` variables, since Herdr starts it in a pane.

Considered: one binary, built from herdr-common, with every app difference in variables. Cargo
builds no binary of a path dependency for its depender (artifact dependencies are unstable), so
each app would build it with `cargo build --manifest-path ../herdr-common/...` before its
tests, outside its own build and its own lock file. And a per-app difference such as reading a
turn's files is code, which a variable cannot hold without the crate taking that app's logic.
