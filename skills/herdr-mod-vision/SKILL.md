---
name: herdr-mod-vision
description: "Visual testing for Herdr (herdr-app) mods, in a private herdr-app: commands, state, screenshots, recordings. Use when working on a herdr-app mod, or asked to show or record one working."
---

Give yourself **eyes** and **hands** on the mod you built, in an **app sandbox**: a private
herdr-app on a headless display, which never shows on the user's display nor takes their focus.
Look before you call the mod done.

- **Hands**: `herdr-app do <mod>.<command>[:<arg>]` lands in the mod's `nextCommand()`.
- **Eyes**: the mod's own state as JSON, and `shot`, the window's real frame.

Everything below runs against the sandbox, through this skill's `app-sandbox` script:
`S=<this skill's directory>/app-sandbox`, set again in each shell command when your shell keeps
no variables. It runs herdr-common's pinned herdr-app and hc; `$S --help` lists its commands,
and `$S tool <program>` runs a tool of herdr-common's dev shell, such as `jq` or `ffmpeg`. The first call builds it, which
takes a minute. Once the sandbox runs, `$S herdr-app <name> --skill` is the reference for mods
and commands.

## 1. Make the mod drivable

Route every user action through one method, used by both the click handler and `nextCommand()`,
so a command exercises the same code path a click does:

```js
init(_props, cx) {
  cx.spawn(async (cx) => {
    for (;;) { this.command(await nextCommand()); cx.notify(); }
  });
}
command(c) {
  const at = c.indexOf(":");
  const [name, arg] = at < 0 ? [c, ""] : [c.slice(0, at), c.slice(at + 1)];
  // ...dispatch on name, then:
  setShared("<mod>.debug", { last: c, /* what the mod shows */ });
}
// in render: .on_click((_e, cx) => { this.command(`toggle:${i}`); cx.notify(); })
```

`<mod>.debug` holds what the mod displays (rows, counts, selection, errors): the text you check
against the picture.

## 2. Start a sandbox and install the mod

Pick a name of your own for the sandbox, such as the mod's: other agents run sandboxes on this
machine.

```sh
$S start <name>                    # prints its directory, inside and display, as JSON
$S mod <name> <mod directory>      # copies the mod in, prints the copy's path inside
$S herdr-app <name> check <the path mod printed>
$S herdr-app <name> do space.new --name vision
$S herdr-app <name> do tab.new --name test --mod <mod> --props '{...}'
```

`mod` installs the mod under its folder's name, which `--mod` and `<mod>.<command>` take. The
sandbox is yours: open spaces, tabs and panes, select and focus at will. It has no network:
drive a mod that fetches with sample data, through its props or commands. `check` renders
once with initial state only, so a typo in a branch that state doesn't reach still passes: the
drive in step 3 catches it.

A mod's code is a copy: after each edit, run `$S mod` again. The app reloads the mod and its
state starts over, so drive again from the start.

## 3. Drive and look

For each behaviour your change touched:

1. **Hands**: `$S herdr-app <name> do "<mod>.add:Buy milk"`. `{"ok": true}` means it was
   delivered; an error means the mod isn't loaded or shown.
2. **Eyes, state**: `$S tool jq '."<mod>.debug"' <app_home>/shared.json`, with the
   `app_home` that `start` printed (`$S info <name>` prints it again). The app saves it in the
   background: read it again until its `last` is the command you sent.
3. **Eyes, pixels**: `$S shot <name> <scratchpad>/step-N.png`, then Read the PNG. Crop it to
   the mod when you need detail: `$S tool ffmpeg -v error -y -i step-N.png -vf crop=W:H:X:Y
   crop.png`, with the coordinates read off the full shot.

A link the mod opens lands in `<directory>/opened.log`, never in a browser. Paths under
`<directory>` are `/proc/<PID>/root/...`, which `cat`, `cp` and `jq` open but Claude Code's Read
does not: copy a file out before you Read it.

The step is done when every behaviour the change touched has been driven, its state matches
what you expected, and the picture matches the state.

## 4. Stop the sandbox

Copy out what you keep from `<directory>`, then `$S stop <name>`: it ends the sandbox
and deletes its files. A sandbox left running stops itself after an hour without a command.

## Record a video

For a run the user wants to watch, record the window, never stitched shots. Run it as one
script, so the recording, the steps and the copy share one shell:

```sh
window=$($S run <name> xdotool search --onlyvisible --name . | head -1)
$S run <name> ffmpeg -v error -y -f x11grab -draw_mouse 0 -window_id "$window" -framerate 30 \
  -i <display> -t <seconds> -vf 'crop=trunc(iw/2)*2:trunc(ih/2)*2' -c:v libx264 \
  -preset ultrafast -crf 18 -pix_fmt yuv420p <inside>/video.mp4 &
# ...the commands of step 3, with `sleep 1.5` after each so a person can follow...
wait
cp <directory>/video.mp4 <where the user wants it>
$S tool ffmpeg -v error -y -ss <t> -i <the copy> -frames:v 1 <scratchpad>/frame.png
```

`<display>` and `<inside>` are what `start` printed: `<inside>` is the sandbox's directory as its
processes see it. `-t` sets the length: cover the whole run, since `stop` waits for a `run` to
end. Read the frame before you hand the video over.
