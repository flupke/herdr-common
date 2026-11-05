# Wiki index

A playbook for working in this repository, one topic per page. `AGENTS.md` holds the rules that
always apply, says what earns a line here, and when to read a page and keep it current.

Elsewhere: the coding standards are in [CODING_STANDARDS.md](../../CODING_STANDARDS.md); the
glossary is [CONTEXT.md](../../CONTEXT.md); decisions are in [.agents/adr/](../adr/); the
settings the skills read are in [.agents/settings/](../settings/).

## Working in the repository

- [Checks](checks.md): gotchas of the check commands.
- [The app sandbox](app-sandbox.md): how `crates/app-sandbox` keeps a private herdr-app off the
  user's display, its bubblewrap gotchas, and how to test a change to it.
- [The herdr-mod-vision skill](mod-vision.md): how the skill and the `app-sandbox` command keep
  a sandbox across shell commands, their gotchas (recording, paths inside), and how to test a
  change to them.
- [The pinned Herdr tools](herdr-tools.md): the herdr, herdr-app and hc releases the tests run,
  how to change one, and how to check a change to the flake.
- [Tests](unit-tests.md): the testing policy, tautological tests, how a test waits.
- [Parallel workspaces](parallel-workspaces.md): the dev shell and the gate in a jj workspace,
  and cleaning up.
- [The fake agent](fake-agent.md): what Herdr reads of it, and how to watch it in a private
  Herdr.
