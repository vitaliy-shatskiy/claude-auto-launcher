# README media

Every image in the README is drawn by a script here. Redraw one only when what it shows changed.

| File | Generator | Source of truth |
|---|---|---|
| `launch-screen.png` | `launch-screen.tape` ([VHS](https://github.com/charmbracelet/vhs)) | the real `claude-auto` binary in preview mode on the demo home |
| `launch-walk.gif`, `launch-walk.mp4` | `launch-walk.tape` (VHS) | the real binary, run interactively on the demo home, quit with `esc` before a launch |
| `sessions-walk.gif`, `sessions-walk.mp4` | `sessions-walk.tape` (VHS) | the real binary in preview mode, one run per step, on the demo home's invented projects and sessions |
| `navigator-walk.gif`, `navigator-walk.mp4` | `navigator-walk.tape` (VHS) | the real binary in preview mode, one run per step, on the demo drive's folders; the create question is answered `esc` |
| `hero-{light,dark}.svg`, `feature-*-{light,dark}.svg` | `draw.mjs` (Node, no dependencies) | the hero's flow is the launcher's screen order and the demo roster's values; the icons are drawings |

The demo home comes from `demo-setup.ps1`: a throwaway `USERPROFILE` under `%TEMP%\claude-auto-demo`,
the roster in `demo-config.json` (three invented accounts), the remembered choices in
`demo-prefs.json`, invented usage numbers (with a Fable weekly figure from a widget record) and
invented projects and sessions from `demo-sessions.json` on a substituted drive (`subst`, dropped
by the next setup and by `render.ps1`). No real config, prefs or projects are read, and
whatever the walk writes (logs, caches) lands in that folder. These files are demo data only: the
product's defaults stay in the code.

Redraw, from the repository root:

```
cargo build --release
pwsh -File docs/media/render.ps1
```

`render.ps1` runs `draw.mjs`, the four tapes, then paints over the cursor block VHS leaves on the screenshot. The tapes need `vhs` with `ttyd` and `ffmpeg` on PATH and the Cascadia Mono font. They take the
release binary under `$env:CARGO_TARGET_DIR` (default `target`), else the debug one. In the walk
the model names and the version in the title come from the `claude.exe` installed on the recording
machine. The preview screenshot reads no catalog and shows the built-in model names. `launch-screen.tape` also writes `target/vhs/launch-screen.gif` (gitignored,
not used).
