# Shipwright

Shipwright is a skill-driven workflow for creating and maintaining idiomatic
language ports. Start with a reference implementation, map its observable
behavior, and build target-language packages checked against shared conformance
fixtures.

The workflow is taking shape in [htomd](https://github.com/jamiedavenport/htomd),
where Python is the source of truth for a TypeScript port:

- `shipwright.toml` identifies the reference source and current target.
- `sw-explore` maps APIs, behavior, tests, and runtime differences.
- `sw-ts` guides TypeScript port creation and updates, including native idioms,
  packaging, and validation.

We're building reusable porting skills around this approach, starting with
Python to TypeScript. This repository currently contains the project overview;
the initial skills and working example live in htomd.
