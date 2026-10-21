//! Portmirage: every service on a quiet, stable, random port, written down in
//! one registry. Milestone 1: allocate, import, check, table, env.

mod registry;
mod rules;

use clap::{Parser, Subcommand};
use registry::{Entry, Registry};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Parser)]
#[command(
    name = "portmirage",
    version,
    about = "One registry for every service port"
)]
struct Cli {
    /// Registry file (default: ~/.config/blackbox/ports.toml)
    #[arg(long, global = true)]
    registry: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Give a service a stable random port (prints the existing one if it has one).
    Allocate {
        service: String,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Record a port a service already uses (pinned), e.g. `import holodrive 8768`.
    Import {
        service: String,
        port: u16,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Compare the registry with live TCP listeners and compose files.
    Check,
    /// Print the registry as a markdown table.
    Table,
    /// Print `PORT_<SERVICE>=n` lines for systemd EnvironmentFile=.
    Env,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let path = cli.registry.unwrap_or_else(registry::default_path);
    let mut reg = match Registry::load(&path) {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    match cli.cmd {
        Cmd::Allocate { service, note } => allocate(&mut reg, &path, &service, &note),
        Cmd::Import {
            service,
            port,
            note,
        } => import(&mut reg, &path, &service, port, &note),
        Cmd::Check => check(&reg),
        Cmd::Table => {
            print!("{}", reg.table());
            ExitCode::SUCCESS
        }
        Cmd::Env => {
            print!("{}", reg.env());
            ExitCode::SUCCESS
        }
    }
}

fn allocate(reg: &mut Registry, path: &Path, service: &str, note: &str) -> ExitCode {
    if !registry::valid_name(service) {
        return fail("service names use [a-z0-9-]");
    }
    if let Some(e) = reg.services.get(service) {
        println!("{}", e.port);
        return ExitCode::SUCCESS;
    }
    let mut taken = reg.ports();
    taken.extend(listening().into_iter().map(|(p, _)| p));
    taken.extend(compose_ports());
    taken.extend(
        std::fs::read_to_string("/etc/services")
            .map(|t| rules::parse_services(&t))
            .unwrap_or_default(),
    );
    let Some(port) = rules::pick(&taken, random_u32, 1000) else {
        return fail("no free port found in the pool");
    };
    reg.services.insert(
        service.into(),
        Entry {
            port,
            note: note.into(),
            pinned: false,
        },
    );
    if let Err(e) = reg.save(path) {
        return fail(&e);
    }
    println!("{port}");
    ExitCode::SUCCESS
}

fn import(reg: &mut Registry, path: &Path, service: &str, port: u16, note: &str) -> ExitCode {
    if let Some((other, _)) = reg
        .services
        .iter()
        .find(|(n, e)| e.port == port && n.as_str() != service)
    {
        return fail(&format!("port {port} already belongs to {other}"));
    }
    reg.services.insert(
        service.into(),
        Entry {
            port,
            note: note.into(),
            pinned: true,
        },
    );
    match reg.save(path) {
        Ok(()) => {
            println!("{service} = {port} (pinned)");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e),
    }
}

fn check(reg: &Registry) -> ExitCode {
    let live = listening();
    let live_ports: BTreeSet<u16> = live.iter().map(|(p, _)| *p).collect();
    let compose = compose_ports();
    let mut problems = 0;
    for (name, e) in &reg.services {
        let state = if live_ports.contains(&e.port) {
            "listening"
        } else {
            "not listening"
        };
        let warn = if rules::EPHEMERAL.contains(&e.port) {
            problems += 1;
            "  ✗ inside the ephemeral range 32768–60999"
        } else {
            ""
        };
        println!("  {:<24} {:>5}  {state}{warn}", name, e.port);
    }
    let registered = reg.ports();
    let mut unregistered: Vec<_> = live
        .iter()
        .filter(|(p, _)| !registered.contains(p) && *p > 1024 && !rules::EPHEMERAL.contains(p))
        .collect();
    unregistered.dedup_by_key(|(p, _)| *p);
    for (p, who) in &unregistered {
        println!("  · unregistered listener {p} {who}");
    }
    for p in compose.intersection(&registered) {
        let owner = reg
            .services
            .iter()
            .find(|(_, e)| e.port == *p)
            .map(|(n, _)| n.as_str())
            .unwrap_or("?");
        if !owner.starts_with("compose") {
            println!("  ✗ port {p} is registered to {owner} but also published by a compose file");
            problems += 1;
        }
    }
    println!(
        "{} registered · {} unregistered listeners · {} problem(s)",
        reg.services.len(),
        unregistered.len(),
        problems
    );
    if problems == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ── live sources ───────────────────────────────────────────────────────────

fn listening() -> Vec<(u16, String)> {
    Command::new("ss")
        .args(["-Hltnp"])
        .output()
        .map(|o| rules::parse_ss(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

fn compose_ports() -> BTreeSet<u16> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut out = BTreeSet::new();
    walk(&home.join("Containers"), 4, &mut |p| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if (n.starts_with("compose") || n.starts_with("docker-compose"))
            && (n.ends_with(".yml") || n.ends_with(".yaml"))
        {
            if let Ok(t) = std::fs::read_to_string(p) {
                out.extend(rules::parse_compose_ports(&t));
            }
        }
    });
    out
}

fn walk(dir: &Path, depth: u8, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, f);
            }
        } else {
            f(&p);
        }
    }
}

fn random_u32() -> u32 {
    let mut b = [0u8; 4];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("read /dev/urandom");
    u32::from_le_bytes(b)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("✗ {msg}");
    ExitCode::FAILURE
}
