![Shipwright](assets/banner.png)

Create and maintain idiomatic language ports from a reference implementation.
Shipwright gives your coding agent the skills to port your code and a CLI to
build, test, compare, and release the results.

Install Shipwright from crates.io (Rust 1.98 or newer):

```sh
cargo install swb --version 0.3.0 --locked
```

The crate is named `swb`; the installed command is `shipwright`. From a project
with `shipwright.toml`, run `shipwright build`, `test`, `lint`, `format`, or `release`
(optionally followed by a package name).
See [CONTRIBUTING.md](CONTRIBUTING.md) to develop it locally. Other commands below
describe planned functionality.

- Import existing projects and keep their tools and conventions.
- Use language skills and dependency mappings to build native APIs.
- Compare behavior through shared fixtures and documented runtime exceptions.
- Update ports as the reference evolves, preserving target-specific work.
- Coordinate builds, checks, benchmarks, versions, and releases.

**Import a project**

```sh
cd htomd
shipwright init --source python/src/htomd
shipwright target add rust --path rust
shipwright target add typescript --path typescript
```

`shipwright.toml` records the reference and targets. For example,
a Python project with a Rust target:

```toml
version = "0.1.1"
source = "python/src/htomd"
targets = ["rust"]
```

**Configure your agent**

```sh
shipwright agents init
```

Installs the relevant skills in `.agents/skills/` and adds Shipwright guidance to
`AGENTS.md`, preserving existing instructions. The guidance establishes the
workflow:

```md
Read shipwright.toml. Treat the configured source as the behavioral reference.
Use sw-explore before creating or updating a port, then the target-language skill.
Preserve observable behavior through idiomatic APIs. Record justified runtime
differences in shared conformance fixtures. Run Shipwright checks after changes.
```

**Explore and port**

Ask your coding agent:

```text
Use sw-explore to map the Python package's APIs, behavior, and tests.
Use sw-rust to create the Rust port, including its library API and CLI.
```

- `sw-explore` maps the behavioral contract and compatibility questions.
- `sw-ts`, `sw-go`, and `sw-rust` guide implementation, idioms, and packaging.
- Dependency mappings help the agent choose equivalents based on actual needs:
  for example, a Python HTTP library may map to a blocking or async Rust client.

**Build and verify**

Commands operate on the reference and all targets by default. Add a package name
to narrow the operation, such as `shipwright build rust` or `shipwright test python`.

```sh
shipwright build
shipwright format
shipwright lint
shipwright test
shipwright lint --fix
shipwright format --fix
```

- `build` runs packages concurrently using `uv build`, `bun run build`,
  Go main-package discovery and builds into `bin/` (or `go build ./...` for libraries),
  or `cargo build --release`. Tooling and
  dependencies must already be installed; outputs stay in their normal locations.
- `format` and `lint` check by default; use `--fix` to apply available fixes.
- `test` runs each package's native tests.

Commands run from each package directory using these defaults:

| Language | Test | Lint | Format check | Format fix |
| --- | --- | --- | --- | --- |
| Python | `uv run pytest` | `uv run ruff check .` | `uv run ruff format --check .` | `uv run ruff format .` |
| TypeScript | `bun run test` | `bun run lint` | `bun run format:check` | `bun run format` |
| Go | `go test ./...` | `go vet ./...` | `gofmt -l .` | `gofmt -w .` |
| Rust | `cargo test` | `cargo clippy --all-targets -- -D warnings` | `cargo fmt --check` | `cargo fmt` |

Lint fixes use Ruff's `--fix`, pass `--fix` to the TypeScript `lint` script,
use `go vet -fix` (requires a Go version supporting that flag), and run
`cargo clippy --fix --allow-dirty --allow-staged -- -D warnings`.
Rust fixes allow uncommitted changes; Cargo's version-control requirements still apply.
Go formatting checks fail when files need formatting, even though `gofmt -l` exits zero.

Commands report results per package and fail if a required check fails or is
missing. Install tooling and dependencies first. These commands do not run a
preliminary build; run `build` first when your package's checks require its outputs.

Planned commands: `typecheck` for ecosystem type checks; `conformance` for shared
fixtures and explicit runtime exceptions; `benchmark` for configured workloads;
and `check` to combine formatting, lint, types, tests, and conformance.

**Keep ports current**

```text
Use sw-rust to update the Rust port for Python changes since v0.1.1.
Preserve independent Rust changes and verify the updated behavior with Shipwright.
```

**Release**

Set the shared `version` in `shipwright.toml` and matching versions in package
manifests, commit your changes, then run:

```sh
shipwright release --dry-run
shipwright release
shipwright release rust
shipwright release --skip-build
```

Release builds and prepares the selected packages concurrently, then publishes
Python distributions to PyPI, an npm tarball to npm, Rust sources to crates.io,
and directory-prefixed Go version tags to `origin`. It does not run tests or
change versions. `--dry-run` permits uncommitted edits and prepares outputs without remote mutations,
while still checking tag and asset conflicts;
`--skip-build` reuses outputs but still packs npm and Cargo sources, skipping
Cargo verification compilation. Keep normal build outputs ignored by Git so
subsequent invocations start with a clean checkout.

Optionally select registry packages and host-platform binary downloads:

```toml
[release]
packages = ["python", "typescript", "go", "rust"] # Default: all configured packages
binaries = ["go", "rust"]                       # Default: none
```

Use `packages = []` for binary downloads only. Go executables stay in `bin/`;
Rust downloads build with an explicit host target in `target/<host>/release/`.
Archives stay beside those outputs. Binary downloads use the GitHub.com `origin`,
a root `v<version>` tag, and a draft release published after success. Existing
notes and published releases are preserved. Matching tags and assets are skipped;
conflicting commits or checksums fail without overwriting anything.

PyPI and npm support Actions trusted publishing with `id-token: write` and
matching trusted-publisher settings. For crates.io, pass the short-lived token
from its authentication action as `CARGO_REGISTRY_TOKEN`. GitHub uploads use the
built-in job token with `contents: write`; no stored registry tokens are needed.

For direct local publishing, configure credentials with `cargo login` (or a Cargo credential provider),
`UV_PUBLISH_TOKEN` (or uv's existing credentials), and `npm login` or a CI npm
token. Interactive npm OTP/browser challenges are handled by Shipwright; CI
must supply credentials that do not require a prompt. GitHub downloads need
`GH_TOKEN` or `GITHUB_TOKEN` with repository contents write access; no `gh` CLI
is needed. Git uses your existing authentication for tag pushes.

Reruns skip exact versions already present on npm and crates.io, without claiming
that local sources match. uv checks individual Python files, allowing partial
wheel/sdist uploads to resume. Successful publications remain intact after
another publisher fails; fix the failure and rerun.

Licensed under the [MIT License](LICENSE).
