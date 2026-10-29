//! What was true of the machine and the tree when a run happened, and where the repository is.
//! Recorded, never acted on -- the one exception is `startup` refusing to run beside a real
//! BertBrowser, and that is its own check. Spec: Infrastructure/EnvironmentProbe.cs, Cli/RepoRoot.cs.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bb_core::benchmarking::{GitInfo, MachineInfo, MachineKey, MachineProbe, Win32Probe};

use crate::args::Fail;

/// The repository root: the directory holding the workspace manifest and `crates/bb-bench`.
pub fn repo_root(override_path: Option<&str>) -> Result<PathBuf, Fail> {
    if let Some(p) = override_path {
        let full =
            std::fs::canonicalize(p).map_err(|e| Fail::Usage(format!("--repo: {p}: {e}")))?;
        let full = strip_verbatim(full);
        if is_root(&full) {
            return Ok(full);
        }
        return Err(Fail::Usage(format!(
            "--repo: no BertBrowser workspace in '{}'.",
            full.display()
        )));
    }
    let starts = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf)),
        std::env::current_dir().ok(),
    ];
    for start in starts.into_iter().flatten() {
        let mut dir = Some(start.as_path());
        while let Some(d) = dir {
            if is_root(d) {
                return Ok(d.to_path_buf());
            }
            dir = d.parent();
        }
    }
    Err(Fail::Error(
        "Could not find the repository above the executable or the current directory; pass --repo <dir>."
            .to_string(),
    ))
}

fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_string_lossy().strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

fn is_root(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file() && dir.join("crates").join("bb-bench").is_dir()
}

/// Everything the commands resolve from the repository, in one place.
#[derive(Clone, Debug)]
pub struct Repo {
    /// The repository root: the workspace manifest, `bench/`, `docs/`, `tools/ui/*.bbs`.
    pub root: PathBuf,
}

impl Repo {
    pub fn find(override_path: Option<&str>) -> Result<Repo, Fail> {
        Ok(Repo {
            root: repo_root(override_path)?,
        })
    }

    pub fn baselines_dir(&self) -> PathBuf {
        self.root.join("bench").join("baselines")
    }

    pub fn results_dir(&self) -> PathBuf {
        self.root.join("bench-results")
    }

    pub fn performance_doc(&self) -> PathBuf {
        self.root.join("docs").join("performance.md")
    }

    pub fn real_config(&self) -> PathBuf {
        self.root.join("bench").join("real.local.json")
    }

    pub fn ui_scripts(&self) -> PathBuf {
        self.root.join("tools").join("ui")
    }

    pub fn baseline_path(&self, machine_key: &str, real: bool) -> PathBuf {
        self.baselines_dir().join(format!(
            "{machine_key}{}.json",
            if real { ".real" } else { "" }
        ))
    }

    pub fn release_out(&self) -> PathBuf {
        self.root.join("out").join("release")
    }
}

pub fn machine(key_override: Option<&str>) -> MachineInfo {
    let facts = MachineKey::probe();
    MachineInfo {
        key: key_override.map_or_else(|| MachineKey::compute(&facts), str::to_string),
        cpu: facts.cpu,
        logical_cores: facts.logical_cores,
        ram_gb: facts.ram_gb,
        os_build: facts.os_build,
        runtime: Win32Probe.runtime_version(),
        power_plan: power_plan(),
        app_running: is_running("BertBrowser.exe"),
        indexer_running: is_running("bb-indexer.exe") || is_running("BertBrowser.Indexer.exe"),
    }
}

pub fn git(repo_root: &Path) -> GitInfo {
    let sha = run("git", &["rev-parse", "HEAD"], Some(repo_root))
        .unwrap_or_else(|| "unknown".to_string());
    let branch = run(
        "git",
        &["rev-parse", "--abbrev-ref", "HEAD"],
        Some(repo_root),
    )
    .unwrap_or_else(|| "unknown".to_string());
    let status = run("git", &["status", "--porcelain"], Some(repo_root));
    GitInfo {
        sha,
        branch,
        dirty: status.is_none_or(|s| !s.is_empty()),
    }
}

/// `Power Scheme GUID: 381b4222-...  (Balanced)` -- the name is the parenthesised part.
fn power_plan() -> Option<String> {
    let text = run("powercfg", &["/getactivescheme"], None)?;
    let end = text.rfind(')')?;
    let start = text[..end].rfind('(')?;
    Some(text[start + 1..end].to_string())
}

/// A tool's stdout, or `None` if it is missing or fails.
fn run(file: &str, args: &[&str], cwd: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new(file);
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether a process with this image name is running.
pub fn is_running(image_name: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    // SAFETY: the snapshot handle is checked and closed; the entry is plain data with dwSize set.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.eq_ignore_ascii_case(image_name) {
                found = true;
                break;
            }
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

/// `yyyyMMdd` of the local date, for the once-a-day database backup.
pub fn local_date_stamp() -> String {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: SYSTEMTIME is plain data and GetLocalTime fills it.
    let t: SYSTEMTIME = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:04}{:02}{:02}", t.wYear, t.wMonth, t.wDay)
}

pub fn tool_version() -> String {
    format!("{} {}", crate::TOOL_NAME, env!("CARGO_PKG_VERSION"))
}
