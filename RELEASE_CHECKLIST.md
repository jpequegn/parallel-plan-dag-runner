# Release Checklist

1. Confirm all repository issues assigned to the release are closed and no pull request is pending.
2. From a clean checkout, run `cargo fmt --all -- --check`.
3. Run `cargo clippy --workspace --all-targets -- -D warnings`.
4. Run `cargo test --workspace` and `cargo build --workspace --locked --release`.
5. Run `plan-runner validate` and `plan-runner run` for all files in `examples/`.
6. Regenerate evaluation output and confirm correct modes pass while every flawed dependency merge is detected.
7. In `web/`, run `npm ci`, `npm test`, and `npm run build`.
8. Serve the production build and smoke-test plan loading, event loading, replay controls, provenance, verifier evidence, comparison view, keyboard focus, and mobile layout.
9. Review `docs/security.md` for any changed tool, authority, persistence, or browser boundary.
10. Confirm `git status --short` is clean and generated reports intended for the release are committed.
11. Tag the verified main commit as `vX.Y.Z` and publish notes covering format compatibility, behavior changes, limitations, and checks.
