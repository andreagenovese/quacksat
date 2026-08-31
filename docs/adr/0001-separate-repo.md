# ADR 0001: Separate repository, not a fork of pollen-robotics/microduck

- Status: accepted
- Date: 2026-08-31

## Context

quacksat runs on the Microduck and talks to `robotd`, the system daemon from
`pollen-robotics/microduck`. Two ways to structure the code were considered:
fork the upstream monorepo and add quacksat inside it, or keep quacksat in
its own repository that depends on upstream crates.

## Decision

quacksat lives in its own repository. It depends on upstream crates
(`duck-ipc-proto`) and, when upstream changes are needed, patches go to
`pollen-robotics/microduck` as pull requests — never as long-lived fork
divergence.

## Consequences

- quacksat has its own release cadence, license header, and issue tracker,
  and stays clearly independent (no implied affiliation with Pollen
  Robotics).
- Upstream refactors (e.g. the planned `mediad` audio migration, milestone
  M5) are consumed as dependency bumps instead of painful fork rebases.
- The `robotd` IPC surface is the only contract quacksat relies on; anything
  not reachable through `/run/robotd.sock` requires an upstream PR first.
- Updates are packaged as a separate systemd unit outside upstream's
  `releases/`, reinstallable via `updaterd`.
