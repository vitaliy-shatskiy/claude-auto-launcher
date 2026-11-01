## What and why

<!-- The problem, then the change. -->

## Verification

<!-- Paste the three gate results. -->

```
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

- [ ] Gates green, output pasted above
- [ ] A test that fails without this change: say which, and what it prints when reverted
- [ ] Touched input or rendering? Name the terminal it was tried in
