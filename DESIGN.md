# Portmirage: design (draft for review)

**Every service on a quiet, stable, random port, recorded in one registry,
with decoys where scanners look.**

The scope was agreed on 2026-09-30 (it started as "port forge").

## Commands

- **`portmirage allocate <service>`:** picks a port above 1024 **outside**
  the ephemeral range 32768–60999, skipping well-known ports, compose host
  ports in `~/Containers` and live listeners. Random but **stable**: it's
  written once to `~/.config/blackbox/ports.toml`, never rotated.
- **Registry:** units and configs read it (`EnvironmentFile=`
  `/run/user/1000/portmirage.env` is rendered from the registry), so
  nothing hard-codes ports.
- **`portmirage check`:** compares the registry with compose files, unit
  files and `ss -ltnp`, reporting drift, conflicts and unregistered
  listeners.
- **`portmirage table`:** a markdown table for the Wiki and the Containers
  README.
- **Later, `portmirage decoy`:** fake listeners on tailnet/LAN-facing
  ports. Any touch is logged, and the IP is handed to **VortexWall** to
  ban, with a **SentryGrid** cross-check so real services are never
  decoyed.

## Milestones

1. Registry + `allocate` + `table` (read-mostly).
2. `check` (compose, units, ss).
3. Migrating existing services onto registry ports, one PR per service,
   with DaemonHall/Holodrive/Nightwatch config generated from the
   registry.
4. Decoys + VortexWall hand-off.

## Decision needed

The original plan was to start after the 12 darkbox services move to
blackbox. Should Milestone 1 (registry, allocate, table) go ahead now,
since it changes nothing about running services, with migration waiting?
Recommended: **yes**.
