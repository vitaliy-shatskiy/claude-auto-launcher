## What and why

<!-- The problem, then the change. -->

## Verification

<!-- Paste the five gate results. -->

```
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo clippy --no-default-features --all-targets --locked -- -D warnings
cargo test --no-default-features --locked
```

- [ ] Gates green, output pasted above
- [ ] A test that fails without this change: say which, and what it prints when reverted
- [ ] Touched input or rendering? Name the terminal it was tried in
