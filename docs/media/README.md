# README media

Every image in the README is drawn by a script here. Redraw one only when what it shows changed.

| File | Generator | Source of truth |
|---|---|---|
| `launch-screen.png` | `launch-screen.tape` ([VHS](https://github.com/charmbracelet/vhs)) | the real `claude-auto` binary in preview mode on the demo home |
| `launch-walk.gif`, `launch-walk.mp4` | `launch-walk.tape` (VHS) | the real binary, run interactively on the demo home, quit with `esc` before a launch |

The demo home comes from `demo-setup.ps1`: a throwaway `USERPROFILE` under `%TEMP%\claude-auto-demo`,
the roster in `demo-config.json` (three invented accounts), the remembered choices in
`demo-prefs.json` and invented usage numbers. No real config, prefs or projects are read, and
whatever the walk writes (logs, caches) lands in that folder. These files are demo data only: the
product's defaults stay in the code.

Redraw, from the repository root:

```
cargo build --release
vhs docs/media/launch-screen.tape
vhs docs/media/launch-walk.tape
```

The tapes need `vhs` with `ttyd` and `ffmpeg` on PATH and the Cascadia Mono font. They take the
release binary under `$env:CARGO_TARGET_DIR` (default `target`), else the debug one. In the walk
the model names and the version in the title come from the `claude.exe` installed on the recording
machine. The preview screenshot reads no catalog and shows the built-in model names. `launch-screen.tape` also writes `target/vhs/launch-screen.gif` (gitignored,
not used).
