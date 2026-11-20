# Speed budget

`budget.json` holds each hot path's work counts; `tests/perf` measures them on every
`cargo test` and fails when one grows past its budget plus `headroomPercent` (10 %). Counts, not
times: the bytes and calls a path reads or writes are exact on any machine under any load, a wall
time is not. Wall times appear only as `wallMsRef`, a reference nobody gates on.

## What is measured

All paths run over one synthetic world (70 sessions in 7 projects, transcripts of 83, 40, 20 and
5 MB among them), built once into `target\tmp\claude-auto-perf-world-v<N>` by `tests/perf/world.rs`.
The real `~\.claude` is never read: the launcher runs with `USERPROFILE` on the world.

| Path | What | Counts |
|---|---|---|
| `startup` | the launcher in preview, start to the launch screen's first frame (the registry scan joined) | the child's `read_bytes`, `read_ops` |
| `project_screen` | the same through Enter, to the project screen's first frame | as above |
| `picker_open_cold` | the picker's first page (pages of 10 until 10 resumable rows), no summary cache | this process's `read_bytes`, `read_ops`, `write_bytes` (the cache write) |
| `picker_open_warm` | the same with the cache an earlier open wrote | as above |
| `picker_cursor` | one Down in the picker at 250x68, colour on | `frame_bytes` the line-diff writer sends, `full_frame_bytes` of the frame before |
| `chat_load` | the chat pane's load of the newest (83 MB) transcript | `chat_bytes_read` (the loader's own count), `read_bytes` |

The I/O counts come from `GetProcessIoCounters` (`src/perf.rs`): every `ReadFile`/`WriteFile` of
the process, pipes and the console included. That is why `tests/perf` is its own test binary and its
path tests take one lock: nothing else may do I/O in that process while a path is measured.

Default deny: a path or metric with no budget, a budgeted metric nothing measures, and a count of 0
(a counter not wired) each fail the check.

## Running the bench

```powershell
cargo build --release --locked --bins --examples
target\release\examples\bench.exe 10          # 10 runs per path: median and p80 ms, plus the counts
target\release\examples\bench.exe 1 --json    # the counts as the "paths" of budget.json
```

The bench times only each path's measured section (a launcher path: spawn to exit). Its counts
equal the test's, except that the child paths may differ by a few KB between the debug exe the test
runs and the release exe the bench runs; the budget holds the test's (debug) counts.

## Updating the budget

- A count went down: lower its budget in the same change (the check would not notice a later
  regression back up).
- A count went up on purpose (a new feature reads more): raise it in the same commit as the code,
  and say in the commit message what the bytes buy. Never raise a budget to make an unexplained
  red go green: find what reads more first.
- Copy the numbers from a failing test's `measured:` line or from `bench.exe 1 --json`.
- A change to the world (`tests/perf/world.rs`) bumps its `VERSION`, so the old world is rebuilt,
  and re-baselines every count.

## A timing run

Wall times are a reference, recorded with the machine state they were taken under in
`budget.json` `timingRun`:

- power plan: `powercfg /getactivescheme`
- mains or battery: `Invoke-RestMethod http://127.0.0.1:18940/api/now` (`grid`, `last.mains`), or
  the error it gave
- CPU load before and after: `(Get-CimInstance Win32_Processor).LoadPercentage`
- the commit, the date (Kyiv), the command

Comparing two builds by time (A/B): interleave A and B runs (A, B, A, B, ...), at least 10 pairs,
and report the median and p80 of the paired differences (B minus A), never two separate medians. A
difference inside the spread of the pairs is "inconclusive", not a result.
