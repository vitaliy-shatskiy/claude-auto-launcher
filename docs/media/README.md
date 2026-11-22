# README media

Every image in the README is drawn by a script here. Redraw one only when what it shows changed.

| File | Generator | Source of truth |
|---|---|---|
| `launch-screen.png` | `launch-screen.tape` ([VHS](https://github.com/charmbracelet/vhs)) | the real `claude-auto` binary, run interactively on the demo home with `CLAUDE_AUTO_DEMO_MANY=1` (about 20 projects, two running sessions); nothing is launched |
| `launch-walk.gif`, `launch-walk.mp4` | `launch-walk.tape` (VHS) | the real binary, run interactively on the demo home, quit with `esc` before a launch |
| `sessions-walk.gif`, `sessions-walk.mp4` | `sessions-walk.tape` (VHS) | the real binary in preview mode, one run per step, on the demo home's invented projects and sessions |
| `navigator-walk.gif`, `navigator-walk.mp4` | `navigator-walk.tape` (VHS) | the real binary in preview mode, one run per step, on the demo drive's folders; the create question is answered `esc` |
| `target/screens/<screen>-<WxH>.png` (not committed) | `screens.ps1` (VHS, one tape per shot) | the real binary in preview mode on the demo home: launch, project, all-projects picker, maintenance, path navigator, rows editor at 280x72, 130x45, 55x20; for looking at, never for the README |
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

`render.ps1` runs `draw.mjs`, then the four tapes. The tapes record a 122-column terminal: the launch page shows as a workspace from 120 columns, and the window has no backdrop margin and 2 px of padding, so those columns fill the image: at the README's 880 px a character cell is about 7.2 px wide (12 px text). Wider padding or a margin shrinks it again. The preview walks hide the cursor through the console (`[Console]::CursorVisible`), since the escape alone does not hold under ConPTY. The tapes need `vhs` with `ttyd` and `ffmpeg` on PATH and the Cascadia Mono font. They take the
release binary under `$env:CARGO_TARGET_DIR` (default `target`), else the debug one. In the still and the launch walk
the model names and the Claude Code version come from the `claude.exe` installed on the recording
machine; the preview walks read no catalog and show the built-in model names.
