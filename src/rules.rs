//! Pure rules: which ports may be allocated, and parsers for the sources of
//! "taken" ports (ss output, compose files, /etc/services).

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Linux's default ephemeral range (`net.ipv4.ip_local_port_range`). Ports in
/// it can be grabbed by any outgoing connection, so services shouldn't live there.
pub const EPHEMERAL: std::ops::RangeInclusive<u16> = 32768..=60999;

/// The quiet pool: unprivileged and outside the ephemeral range.
pub fn in_pool(p: u16) -> bool {
    p > 1024 && !EPHEMERAL.contains(&p)
}

/// Pick a port from the pool that isn't taken, driven by `rand` (any source of
/// randomness; tests pass a fixed sequence). Returns None if `tries` draws
/// all collide (the pool is ~37k ports, so in practice it never happens).
pub fn pick(taken: &BTreeSet<u16>, mut rand: impl FnMut() -> u32, tries: usize) -> Option<u16> {
    // Pool = 1025..=32767 (31743 ports) ∪ 61000..=65535 (4536 ports).
    const LOW: u32 = 32767 - 1025 + 1;
    const HIGH: u32 = 65535 - 61000 + 1;
    for _ in 0..tries {
        let n = rand() % (LOW + HIGH);
        let port = if n < LOW { 1025 + n } else { 61000 + (n - LOW) } as u16;
        debug_assert!(in_pool(port));
        if !taken.contains(&port) {
            return Some(port);
        }
    }
    None
}

/// TCP listening ports from `ss -Hltn` (local address column), with process names
/// when `-p` output is present.
pub fn parse_ss(out: &str) -> Vec<(u16, String)> {
    static PROC: OnceLock<Regex> = OnceLock::new();
    let proc_re = PROC.get_or_init(|| Regex::new(r#"\(\("([^"]+)""#).unwrap());
    let mut v = Vec::new();
    for line in out.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let Some(local) = cols.get(3) else { continue };
        let Some(port) = local.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) else {
            continue;
        };
        let who = proc_re
            .captures(line)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        v.push((port, who));
    }
    v.sort();
    v.dedup();
    v
}

/// Host ports published by a compose file: short syntax ("8080:80",
/// "127.0.0.1:8080:80/tcp", "8000-8002:8000-8002") and long syntax (`published: 8080`).
pub fn parse_compose_ports(yaml: &str) -> BTreeSet<u16> {
    static SHORT: OnceLock<Regex> = OnceLock::new();
    static LONG: OnceLock<Regex> = OnceLock::new();
    let short = SHORT.get_or_init(|| {
        Regex::new(r#"(?m)^\s*-\s*["']?(?:\[?[0-9a-fA-F.:]+\]?:)?(\d{1,5})(?:-(\d{1,5}))?:\d{1,5}(?:-\d{1,5})?(?:/(?:tcp|udp))?["']?\s*$"#).unwrap()
    });
    let long =
        LONG.get_or_init(|| Regex::new(r#"(?m)^\s*published:\s*["']?(\d{1,5})["']?\s*$"#).unwrap());
    let mut out = BTreeSet::new();
    for c in short.captures_iter(yaml) {
        let a: u16 = c[1].parse().unwrap_or(0);
        let b: u16 = c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(a);
        out.extend(a..=b.max(a));
    }
    for c in long.captures_iter(yaml) {
        if let Ok(p) = c[1].parse() {
            out.insert(p);
        }
    }
    out.remove(&0);
    out
}

/// TCP ports named in /etc/services (so allocations avoid well-known ports).
pub fn parse_services(text: &str) -> BTreeSet<u16> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|pp| pp.strip_suffix("/tcp"))
        .filter_map(|p| p.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_excludes_privileged_and_ephemeral() {
        assert!(!in_pool(80) && !in_pool(1024) && in_pool(1025));
        assert!(
            in_pool(32767)
                && !in_pool(32768)
                && !in_pool(60999)
                && in_pool(61000)
                && in_pool(65535)
        );
    }

    #[test]
    fn pick_skips_taken_and_stays_in_pool() {
        let taken: BTreeSet<u16> = [1025, 1026].into();
        let mut seq = [0u32, 1, 2].into_iter();
        assert_eq!(pick(&taken, || seq.next().unwrap(), 5), Some(1027));
        // n just past the low range maps into the high range.
        let mut hi = [32767 - 1025 + 1].into_iter();
        assert_eq!(
            pick(&BTreeSet::new(), || hi.next().unwrap(), 1),
            Some(61000)
        );
        // every draw collides → None
        assert_eq!(pick(&[1025].into(), || 0, 3), None);
        for x in [0u32, 7, 99_999, u32::MAX] {
            assert!(in_pool(pick(&BTreeSet::new(), || x, 1).unwrap()));
        }
    }

    #[test]
    fn parses_ss() {
        let out = "LISTEN 0 1024 127.0.0.1:8768 0.0.0.0:* users:((\"holodrive\",pid=1,fd=9))\nLISTEN 0 128 [::]:22 [::]:*\nLISTEN 0 128 127.0.0.53%lo:53 0.0.0.0:*\n";
        assert_eq!(
            parse_ss(out),
            vec![
                (22, String::new()),
                (53, String::new()),
                (8768, "holodrive".into())
            ]
        );
    }

    #[test]
    fn parses_compose_short_and_long_syntax() {
        let y = r#"
services:
  web:
    ports:
      - "8080:80"
      - 127.0.0.1:2586:80/tcp
      - '9000-9002:9000-9002'
      - "[::1]:7070:7070"
      - "3000"
  api:
    ports:
      - target: 80
        published: "8443"
    environment:
      - TZ=America/New_York
"#;
        assert_eq!(
            parse_compose_ports(y),
            [2586, 7070, 8080, 8443, 9000, 9001, 9002].into()
        );
    }

    #[test]
    fn parses_etc_services() {
        let s = "# comment\nssh 22/tcp\nssh 22/udp\nhttp-alt 8080/tcp webcache\n";
        assert_eq!(parse_services(s), [22, 8080].into());
    }
}
