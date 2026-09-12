# Contributing

Install [mise](https://mise.jdx.dev/getting-started.html), then run from the repository root:

```sh
mise trust
mise install
mise run dev
```

Mise pins Rust and installs rustfmt and Clippy. Cargo downloads dependencies on
the first run; keep `Cargo.lock` committed. Shell activation is not required.

Pass CLI arguments directly:

```sh
mise run dev --help
mise run dev --version
```

The CLI supports parallel package builds and releases. Other commands in the README are planned.

Before submitting changes:

```sh
mise run format
mise run check
```

`check` runs formatting checks, Clippy (including type checking), and tests.
Use `mise run lint` or `mise run test` to run those checks individually.

Build and run a standalone binary:

```sh
mise run build
./target/release/shipwright --help
```

On Windows, the binary is `target/release/shipwright.exe`.

To try the build against a sibling htomd checkout after building Shipwright:

```sh
cd ../htmd
../shipwright/target/release/shipwright build
../shipwright/target/release/shipwright build rust
```

Package names are `python`, `typescript`, `go`, and `rust`. Shipwright reads
`shipwright.toml`, locates the source manifest by walking upward from its source
path, and finds targets in their named directories. Each build keeps its normal
output location. Failures report a short excerpt and a full log path; logs are
retained in a temporary directory. Ctrl-C stops active builds.

For manual verification, build all four packages and then each individually;
try a failing build and Ctrl-C to inspect their reporting. Release verification is manual: exercise dry runs, repeat releases, npm OTP and
browser authentication, partial failures, and Ctrl-C in a disposable project.

Start in `src/main.rs`. Keep changes focused and add modules or dependencies
when a feature needs them. Add regression tests for new behavior.

Releases are published manually to crates.io as `swb`; the binary remains
`shipwright`. Update the version in `Cargo.toml` and the README installation
command, then refresh `Cargo.lock` with `cargo check`. From a reviewed checkout:

```sh
mise run check
cargo publish --locked --registry crates-io --dry-run
cargo publish --locked --registry crates-io
```

Cargo uses the credentials configured by `cargo login`. Verify the release with
`cargo install swb --version <version> --locked` and `shipwright --version`.
