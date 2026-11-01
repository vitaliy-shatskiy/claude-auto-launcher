# README media

Every image in the README is drawn by a script here. Redraw one only when what it shows changed.

| File | Generator | Source of truth |
|---|---|---|
| `launch-screen.png` | `launch-screen.tape` ([VHS](https://github.com/charmbracelet/vhs)) | the real `claude-auto-rs` binary in preview mode on `tests/fixtures/config-four.json` |

Redraw, from the repository root:

```
cargo build --release
vhs docs/media/launch-screen.tape
```

The tape needs `vhs` with `ttyd` and `ffmpeg` on PATH and the Cascadia Mono font. It takes the
release binary under `$env:CARGO_TARGET_DIR` (default `target`), else the debug one. It runs with
`CLAUDE_AUTO_PREVIEW=1`, a throwaway `USERPROFILE` and prefs file under `%TEMP%\claude-auto-demo`,
and one `Escape` key, so nothing is launched and no real prefs or projects are read. The model names on the
model row come from the `claude.exe` installed on the recording machine. VHS also writes
`target/vhs/launch-screen.gif` (gitignored, not used).
