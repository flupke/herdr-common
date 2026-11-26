# What a Herdr server reads of a fake agent

Checked on Herdr 0.9.5, with the fake agent in a private server's pane.

- Herdr detects the agent by its process's name: run it through a link named `claude` (or
  `codex`), and Herdr gives the pane `agent: claude`.
- Its status comes from the pane's title: `✳ ` first is `idle`, a braille spinner first is
  `working`.
- Claude Code's permission form after the last rule (`─…`) is `blocked`; the input line between
  rules that follows it is `idle` again.
- `pane.agent_status_changed` reports `working` once Herdr saw it; `agent.get` gives the status
  at the moment it is asked. Subscribe on `HERDR_SOCKET_PATH` before the agent shows anything,
  or the event may come first.
- Herdr's events name an event with a dot (`pane.agent_status_changed`) or an underscore
  (`pane_agent_detected`): read both.
