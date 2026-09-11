![Shipwright](assets/banner.png)

Create and maintain idiomatic language ports from a reference implementation.
Shipwright gives your coding agent the skills to port your code and a CLI to
build, test, compare, and release the results.

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

`shipwright.toml` records the reference, targets, and project commands. For example,
a Python project with a Rust target:

```toml
version = "0.1.1"
source = "python/src/htomd"
targets = ["rust"]

[commands.python]
build = "mise run build:python"
test = "mise run test:python"

[commands.rust]
build = "cargo build --manifest-path rust/Cargo.toml --release"
format = "cargo fmt --manifest-path rust/Cargo.toml --check"
lint = "cargo clippy --manifest-path rust/Cargo.toml -- -D warnings"
test = "cargo test --manifest-path rust/Cargo.toml"
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
shipwright typecheck
shipwright test
shipwright conformance
shipwright benchmark
shipwright check
```

- `build` builds each package using its configured tooling.
- `format`, `lint`, and `typecheck` run the corresponding ecosystem tools;
  `format` checks formatting by default, with `--write` to apply changes.
- `test` runs each package's native tests.
- `conformance` compares every target against the reference using shared fixtures
  and explicit runtime exceptions. `shipwright conformance rust` checks Rust alone.
- `benchmark` compares configured workloads across packages.
- `check` combines formatting, linting, type checks, tests, and conformance.

Commands report results per package and fail if a required check fails or is
missing. Checks use existing build artifacts; run `build` first when needed.

**Keep ports current**

```text
Use sw-rust to update the Rust port for Python changes since v0.1.1.
Preserve independent Rust changes and verify the updated behavior with Shipwright.
```

**Version and deploy**

```sh
shipwright version set 0.2.0
shipwright deploy --dry-run
shipwright deploy
```

- `version set` updates versions across configured packages.
- `deploy --dry-run` validates release artifacts and previews publishing actions.
- `deploy` publishes all configured packages through the project's existing
  workflows. Use `shipwright deploy rust` to publish a single package.
