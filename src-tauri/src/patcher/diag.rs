//! Environment and log diagnostics for the GUI.
//!
//! Covers host/Wine checks, per-session game-log network verdicts, and process
//! detection. Nothing here is macOS-privileged except the *suggested* hostname
//! fix, which is deliberately never executed -- see `EnvReport::fix_command`.

use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

/// Where CrossOver keeps its bundled Wine.
pub const CX_BIN: &str = "/Applications/CrossOver.app/Contents/SharedSupport/CrossOver/bin";

pub const DEFAULT_BOTTLE: &str = "Steam";

/// The game's own log, used for the per-session network verdict.
pub fn python_log_path(install_root: &Path) -> PathBuf {
    install_root.join("profile").join("python.log")
}

// Environment --------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HostnameFact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvReport {
    /// `scutil --get HostName` -- must end in `.local`, or the game cannot work
    /// out its own IP address.
    pub host_name: String,
    /// `scutil --get LocalHostName` -- the mDNS name; must not be touched, it
    /// is what makes `.local` resolve.
    pub local_host_name: String,
    /// The plain `hostname` command.
    pub unix_hostname: String,
    /// Address the unix hostname resolves to, if any.
    pub resolved_address: Option<String>,
    /// Wine's own copy of the hostname, which it rewrites on every start.
    pub wine_registry: Option<String>,
    /// Which interface traffic to 8.8.8.8 actually leaves by.
    pub egress_interface: Option<String>,
    /// True when the hostname is correct and resolves.
    pub hostname_ok: bool,
    /// True when the egress interface looks like a VPN tunnel.
    pub vpn_warning: bool,
    /// The one-line fix, present only when `hostname_ok` is false.
    pub fix_command: Option<String>,
    /// Approaches that are known not to work, so nobody retries them.
    pub dead_ends: Vec<String>,
    /// Extra notes worth showing verbatim.
    pub notes: Vec<String>,
}

/// Run a command, capturing stdout and falling back to stderr.
///
/// The shell original used `2>&1`; `scutil --get HostName` prints its "not
/// set" complaint on stderr, so both streams matter.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Some(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        None
    } else {
        Some(stderr)
    }
}

/// Resolve a hostname the way the game's own `gethostname()` + resolve does.
///
/// The whole bug this tool exists to fix is that macOS cannot resolve a bare
/// hostname -- `ping UserdeMacBook-Pro` fails, while `ping
/// UserdeMacBook-Pro.local` succeeds via mDNS.
fn resolve(hostname: &str) -> Option<String> {
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return None;
    }
    let mut addresses = (hostname, 0u16).to_socket_addrs().ok()?;
    addresses.next().map(|address| address.ip().to_string())
}

fn egress_interface() -> Option<String> {
    let text = run("route", &["-n", "get", "8.8.8.8"])?;
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(|value| value.trim().to_string())
    })
}

/// Query Wine's copy of the hostname.
///
/// This starts a `wineserver`, so it is only ever called on an explicit user
/// action -- never on a timer and never during status refresh.
pub fn wine_registry_hostname(bottle: &str) -> Option<String> {
    let wine = Path::new(CX_BIN).join("wine");
    if !wine.is_file() {
        return None;
    }
    let output = Command::new(&wine)
        .args([
            "reg",
            "query",
            r"HKLM\System\CurrentControlSet\Services\Tcpip\Parameters",
            "/v",
            "Hostname",
        ])
        .env("CX_BOTTLE", bottle)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    stdout
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("hostname"))
        .map(|line| line.trim().to_string())
}

pub fn env_report(include_wine: bool, bottle: &str) -> EnvReport {
    let host_name = run("scutil", &["--get", "HostName"]).unwrap_or_default();
    let local_host_name = run("scutil", &["--get", "LocalHostName"]).unwrap_or_default();
    let unix_hostname = run("hostname", &[]).unwrap_or_default();

    let resolved_address = resolve(&unix_hostname);
    let egress_interface = egress_interface();

    // The four facts that must all be right for the game to get a socket.
    let host_name_is_local = host_name.ends_with(".local");
    let hostname_ok = host_name_is_local && resolved_address.is_some();

    let mut notes = Vec::new();
    let fix_command = if host_name_is_local {
        None
    } else if local_host_name.is_empty() {
        notes.push(
            "读不到 LocalHostName，无法自动推导修复命令。请先确认 \
             `scutil --get LocalHostName` 能返回一个名字。"
                .to_string(),
        );
        None
    } else {
        Some(format!(
            "sudo scutil --set HostName {}.local",
            local_host_name
        ))
    };

    if !host_name_is_local {
        notes.push(
            "HostName 不以 .local 结尾。游戏会调用 gethostname() 拿到这个名字，\
             再把它解析成自己的本地 IP —— 而 macOS 解析不了一个裸主机名，\
             于是 socket 绑定失败，游戏报网络错误。"
                .to_string(),
        );
    } else if resolved_address.is_none() {
        notes.push(format!(
            "HostName 看起来是对的，但 {} 解析不出地址。请确认 mDNS 正常。",
            unix_hostname
        ));
    }

    let vpn_warning = egress_interface
        .as_deref()
        .is_some_and(|interface| interface.starts_with("utun"));

    EnvReport {
        host_name,
        local_host_name,
        unix_hostname,
        resolved_address,
        wine_registry: if include_wine {
            wine_registry_hostname(bottle)
        } else {
            None
        },
        egress_interface,
        hostname_ok,
        vpn_warning,
        fix_command,
        dead_ends: vec![
            "在 /etc/hosts 里写死 \"<当前IP> <主机名>\"：确实能用，但依赖 DHCP，\
             每次换网络就失效，而且这台机器上有东西会重写 /etc/hosts。"
                .to_string(),
            "改 Wine 的 HKLM\\System\\CurrentControlSet\\Services\\Tcpip\\Parameters\\\
             Hostname（无论用 reg add 还是直接改 system.reg）：Wine 每次启动 \
             wineserver 都会从 Unix 主机名重新生成这个值（去点、转小写），永远不会持久化。"
                .to_string(),
        ],
        notes,
    }
}

// Log ----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LogSession {
    /// `HH:MM:SS` of the session's `starting on` line.
    pub started: String,
    pub errors: u64,
    /// Failures of `Endpoint::getLocalAddress`. Anything above zero means the
    /// game could not determine its own address, so networking is dead.
    pub local_address_failures: u64,
    /// Rendered verdict, so the UI does not re-implement the rule.
    pub verdict: String,
    pub healthy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogReport {
    pub path: String,
    pub exists: bool,
    pub size: u64,
    pub modified: Option<String>,
    pub sessions: Vec<LogSession>,
}

fn timestamp_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(\d{2}:\d{2}:\d{2}) \d{4}$").expect("valid timestamp regex")
    })
}

/// Per-session error counts from the game's `python.log`.
///
/// The key metric is `getLocalAddress`: anything above zero means the game
/// failed to determine its own local address, so networking is necessarily
/// unavailable.
pub fn log_report(install_root: &Path) -> LogReport {
    let path = python_log_path(install_root);
    let mut report = LogReport {
        path: path.display().to_string(),
        exists: false,
        size: 0,
        modified: None,
        sessions: Vec::new(),
    };

    let Ok(metadata) = std::fs::metadata(&path) else {
        return report;
    };
    report.exists = true;
    report.size = metadata.len();
    report.modified = metadata.modified().ok().map(|time| {
        let time: chrono::DateTime<chrono::Local> = time.into();
        time.format("%m-%d %H:%M:%S").to_string()
    });

    let Ok(text) = std::fs::read_to_string(&path) else {
        return report;
    };

    // A line both starts a session *and* can carry ERROR/getLocalAddress; the
    // original awk does not `next` after matching, so neither do we.
    let mut current: Option<usize> = None;
    for line in text.lines() {
        if line.contains("starting on ") {
            if let Some(captures) = timestamp_pattern().captures(line) {
                if let Some(stamp) = captures.get(1) {
                    report.sessions.push(LogSession {
                        started: stamp.as_str().to_string(),
                        errors: 0,
                        local_address_failures: 0,
                        verdict: String::new(),
                        healthy: true,
                    });
                    current = Some(report.sessions.len() - 1);
                }
            }
        }
        if let Some(index) = current {
            let session = &mut report.sessions[index];
            if line.contains("ERROR") {
                session.errors += 1;
            }
            if line.contains("getLocalAddress") {
                session.local_address_failures += 1;
            }
        }
    }

    for session in &mut report.sessions {
        session.healthy = session.local_address_failures == 0;
        session.verdict = if session.healthy {
            "正常".to_string()
        } else {
            "网络不可用".to_string()
        };
    }

    report
}

// Processes ----------------------------------------------------------------

/// Game and launcher processes that make patching unsafe.
///
/// The shell original used `pgrep -ifl 'WorldOfWarships|steam\.exe'` and only
/// warned -- it never blocked. That stays true: deciding is the user's job.
///
/// Shelling out keeps `sysinfo` (and its dependency tree) out of the build.
/// `pgrep -ifl` prints `pid name args`; the pid is dropped for display.
pub fn running_processes() -> Vec<String> {
    let Ok(output) = Command::new("pgrep")
        .args(["-ifl", r"WorldOfWarships|steam\.exe"])
        .output()
    else {
        return Vec::new();
    };

    // pgrep exits 1 when nothing matches, which is not an error here.
    let mut found: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            parts.next()?; // pid
            let rest: Vec<&str> = parts.collect();
            (!rest.is_empty()).then(|| rest.join(" "))
        })
        .collect();
    found.sort();
    found.dedup();
    found
}
