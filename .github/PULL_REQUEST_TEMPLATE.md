## What and why

<!-- The problem, then the change. -->

## Verification

<!-- Paste the checkpoint table. Three rows need a machine-local reference recorded once
     (`check-regression.ps1 -Record`, `check-preview.ps1 -Record`); `2 DID NOT RUN` is not a pass. -->

```
pwsh -File tests\checkpoint.ps1
```

- [ ] Checkpoint green, table pasted above
- [ ] A test that fails without this change — say which, and what it prints when reverted
- [ ] Touched input or rendering? Name the terminal it was tried in
