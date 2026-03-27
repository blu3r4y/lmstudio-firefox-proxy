# Contributing

Thanks for your interest in contributing!

## Getting started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/<your-username>/lmstudio-firefox-proxy`
3. Create a branch: `git checkout -b my-feature`
4. Make your changes
5. Run checks: `cargo fmt --check && cargo clippy -- -D warnings`
6. Commit and push
7. Open a Pull Request

## Development

Requires [Rust](https://rustup.rs/) 1.85+.

```sh
cargo build          # dev build
cargo run            # run locally
cargo clippy         # lint
cargo fmt            # format
```

## Code style

- Run `cargo fmt` before committing
- All clippy warnings must be resolved
- Keep the codebase simple — this is a small, focused tool

## Releasing

To create a new release:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The [Release workflow](.github/workflows/release.yml) will automatically build binaries for all platforms and create a GitHub Release.
