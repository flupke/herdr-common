# The vision core

What `crates/vision-mcp`, `vision-display` and `vision-signal` do not say in their code: how
herdr-app and hc behave under them, and how to test a change. ADR 0003 says why `vision-signal`
is the one crate apps link.

## How an action knows the app reacted

Every input goes through the sandbox's X display (XTEST), as a person's keyboard and mouse,
so keys, typing, clicks and drags reach a terminal or a mod alike, in the order given.

- The focused terminal's program speaks vision-signal once hc has reported one frame marker
  for it. The vision core then pastes an acknowledgement request after the input, through the same
  window: the display's clipboard and `ctrl+shift+v` (herdr-app's `terminal.paste`). Sent
  through hc instead, the request could reach the program before the input the window is
  still handing on.
- An input that moves the focus off that terminal, such as a click on another pane, gets its
  reaction from the pixels: the vision core asks `herdr-app status` for the focus after the input,
  before it pastes. The window has handled the X input by then in every run so far.
- An input that makes the program quit may leave the request typed at its shell's prompt: the
  program's `FrameMarker::RESET` title tells the vision core it left, too late to hold the paste
  back. hc's `foreground` events are no sign of it: hc reports a program's arrival after its
  first frames at times.
- Any other focus (a shell, a mod) is judged by the window's pixels (`Reaction::Pixels`).
- While a held drag keeps the left button down, an input is judged by the window's pixels,
  with no acknowledgement request: the paste key would leave a drag on herdr-app's panels
  unable to drop (below).
- `Vision::settle` is the same wait with no input; `settle_pixels` judges by the pixels alone.

## herdr-app and hc, as the vision core meets them

Checked on herdr-app 0.1.13 and hc 0.1.18:

- `hc keys` returns once the keys reached the terminal: an acknowledgement request pasted
  after it reaches the program after them (the probe's settle test). `hc send` is untested so.
- `hc events --json` prints a snapshot line (`terminals`, each with its `title`), then one
  `{"method": "event", "params": {"terminal", "type", "data"}}` line per change; a title change
  is `type: title` with `data.title`, after the bytes before it.
- `hc read <t> --json` gives the screen as `text`, trailing empty rows dropped.
- `herdr-app status` names the `focused_pane`; `herdr-app ls` maps panes to terminals. A mod's
  pane maps to none.
- herdr-app sends a program the mouse reports of clicks and drags without their modifiers: a
  Ctrl or Alt the vision core holds through them never shows in the program's reports.
- A paste key pressed while a held drag keeps the left button down leaves a drag on
  herdr-app's own panels (the Spaces list) unable to drop: the release that follows changes
  nothing. A program's drag survives it.
- In `herdr-app do <command> '<json>'`, `argv` is a JSON array; a string is ignored, and the
  terminal runs a shell.
- `hc events` ends with its core: `Vision::restart` starts a new one after a core restart or
  a reboot.
- In bubblewrap, `hc events` runs under nsenter, which forks: killing the nsenter leaves it
  running until the sandbox stops, which is why `Vision::stop` drops the event watch last.

## Testing a change

`make test CRATES=vision-mcp` drives `vision-probe` (a binary of the crate, std and
vision-signal only, in `stty raw` mode) and a shell in the sandbox through the MCP tools. The
probe paints its reaction late on purpose, so a vision core that reads the screen before the
acknowledging frame fails the test. The shell's settle releases a `read` that waits with no
echo, then prints half a second later: nothing shows before the wait starts, so a settle that
skips the change-wait fails. A change to the display input also needs the run beside the
user's session in [The app sandbox](app-sandbox.md) (Testing a change), with
`CRATES=vision-mcp`.
