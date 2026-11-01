## What and why

<!-- The problem, then the change. Which launcher: Rust, PowerShell, or both. -->

## Verification

<!-- Rust: paste the three gate results. PowerShell: paste the checkpoint table. Three checkpoint
     rows need a machine-local reference recorded once (`check-regression.ps1 -Record`,
     `check-preview.ps1 -Record`). `2 DID NOT RUN` is not a pass. -->

```
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
pwsh -File tests\checkpoint.ps1
```

- [ ] Gates green for the launcher(s) touched, output pasted above
- [ ] A test that fails without this change: say which, and what it prints when reverted
- [ ] Touched input or rendering? Name the terminal it was tried in
