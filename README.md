# 🪞 Portmirage

**Every service on a quiet, stable, random port, written down in one registry. Later: decoys where scanners look.**

Hard-coded ports drift: a service's port lives in its config, a systemd unit, a compose file, a Tailscale serve rule and three READMEs. Portmirage keeps **one** registry (`~/.config/blackbox/ports.toml`) that everything reads, and picks new ports where nothing else will land.

## 🎲 Milestone 1

| Command | What it does |
|---|---|
| `portmirage allocate <svc> [--note]` | A random port that stays the same once chosen: above 1024, **outside the ephemeral range 32768–60999**, and not in `/etc/services`, your `~/Containers` compose files, the registry or live listeners. Asking again returns the same port. |
| `portmirage import <svc> <port>` | Records a port a service already uses (pinned). Refuses a port that belongs to another service. |
| `portmirage check` | Registry vs live TCP listeners and compose files: what's listening, what isn't, unregistered listeners, and conflicts. Exits non-zero on problems. |
| `portmirage table` | A markdown table for the Wiki and the Containers README. |
| `portmirage env` | `PORT_<SERVICE>=n` lines for a systemd `EnvironmentFile=`. |

```toml
# ~/.config/blackbox/ports.toml
[services.holodrive]
port = 8768
pinned = true
```

## 🗺️ Roadmap

See [DESIGN.md](DESIGN.md):

1. ✅ Registry + allocate/import/check/table/env.
2. Render the env file at login, and give `check` a timer that reports to Warnflare.
3. Migrate services onto the registry, one PR per service.
4. **Decoys:** fake listeners where scanners probe. A touch hands the IP to **VortexWall**, and **SentryGrid** confirms no real service is ever decoyed.

MIT licensed.
