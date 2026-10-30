//! Names the machine a baseline belongs to, so a comparison is always against the same hardware.
//! The key is readable on purpose — `ryzen-9-7950x-32c-64gb-w26200-net10.0-3f9a1c2b` says what it
//! is in a directory listing — with a short hash on the end so two machines whose slugs collide
//! still file separately. Only the probe touches the OS; `compute` is pure, and that is what the
//! tests pin. Spec: Core/Benchmarking/MachineKey.cs.

use std::sync::LazyLock;

use fancy_regex::Regex;
use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// The facts a machine key is made from. Nothing here changes between reboots.
/// `runtime_major_minor` is `1.99`, not `1.99.1`: a patch release must not orphan every baseline on
/// the machine. The full version is recorded beside the key in `MachineInfo::runtime`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineFacts {
    pub cpu: String,
    pub logical_cores: u32,
    pub ram_gb: u32,
    pub os_build: u32,
    pub runtime_major_minor: String,
}

/// Where the facts come from, so `probe_with` stays testable without a machine.
pub trait MachineProbe {
    fn cpu_name(&self) -> Option<String>;
    fn logical_cores(&self) -> u32;
    fn total_memory_bytes(&self) -> u64;
    fn os_build(&self) -> u32;
    /// The full version of what runs the code. A native build has no runtime; the compiler that
    /// built it is what changes the numbers the same way a runtime patch did.
    fn runtime_version(&self) -> String;
}

/// Long enough for "amd-ryzen-9-7950x-16-core" whole; a file name, not a sentence.
const MAX_SLUG_LENGTH: usize = 32;

static VENDOR_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\((r|tm|c)\)|\b(processor|cpu|with radeon graphics)\b").expect("regex")
});
static CLOCK_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)@\s*[\d.]+\s*ghz").expect("regex"));
static NON_ALPHANUMERIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^a-z0-9]+").expect("regex"));

pub struct MachineKey;

impl MachineKey {
    pub fn compute(facts: &MachineFacts) -> String {
        let canonical = format!(
            "{}|{}|{}|{}|{}",
            facts.cpu.trim(),
            facts.logical_cores,
            facts.ram_gb,
            facts.os_build,
            facts.runtime_major_minor
        );
        let digest = Sha256::digest(canonical.as_bytes());
        let hash: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();

        format!(
            "{}-{}c-{}gb-w{}-net{}-{hash}",
            Self::slug(&facts.cpu),
            facts.logical_cores,
            facts.ram_gb,
            facts.os_build,
            facts.runtime_major_minor
        )
    }

    /// A CPU name reduced to something that can be a file name: lowercase, vendor noise dropped,
    /// runs of anything that is not a letter or digit collapsed to one dash.
    pub fn slug(cpu: &str) -> String {
        let text = cpu.to_lowercase();
        let text = VENDOR_NOISE.replace_all(&text, " ");
        let text = CLOCK_SUFFIX.replace_all(&text, " ");
        let text = NON_ALPHANUMERIC.replace_all(&text, "-");
        let mut text = text.trim_matches('-').to_string();

        if text.len() > MAX_SLUG_LENGTH {
            text.truncate(MAX_SLUG_LENGTH);
            text = text.trim_end_matches('-').to_string();
        }

        if text.is_empty() {
            "cpu".to_string()
        } else {
            text
        }
    }

    /// Reads the facts off the running machine.
    pub fn probe() -> MachineFacts {
        Self::probe_with(&Win32Probe)
    }

    pub fn probe_with(probe: &dyn MachineProbe) -> MachineFacts {
        let cpu = probe
            .cpu_name()
            .unwrap_or_else(|| "unknown-cpu".to_string());
        let ram_gb =
            (probe.total_memory_bytes() as f64 / (1024.0 * 1024.0 * 1024.0)).round() as u32;
        let runtime = probe.runtime_version();
        let mut parts = runtime.split('.');
        let major = parts.next().unwrap_or("0");
        let minor = parts.next().unwrap_or("0");

        MachineFacts {
            cpu,
            logical_cores: probe.logical_cores(),
            ram_gb,
            os_build: probe.os_build(),
            runtime_major_minor: format!("{major}.{minor}"),
        }
    }
}

/// The real machine, through Win32.
pub struct Win32Probe;

impl Win32Probe {
    /// The compiler this crate was built with, baked in by `build.rs`.
    pub const RUSTC_VERSION: &'static str = env!("BB_RUSTC_VERSION");
}

impl MachineProbe for Win32Probe {
    fn cpu_name(&self) -> Option<String> {
        let value = read_registry_string(
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "ProcessorNameString",
        )?;
        // The registry value is padded with trailing spaces on some machines.
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    fn logical_cores(&self) -> u32 {
        std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
    }

    fn total_memory_bytes(&self) -> u64 {
        // SAFETY: MEMORYSTATUSEX is plain data; dwLength is set as the API requires.
        unsafe {
            let mut status: MEMORYSTATUSEX = std::mem::zeroed();
            status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut status) == 0 {
                0
            } else {
                status.ullTotalPhys
            }
        }
    }

    fn os_build(&self) -> u32 {
        use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
        use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
        // RtlGetVersion rather than GetVersionExW: the latter answers through the manifest's
        // compatibility shims and can name an older Windows than the one running.
        // SAFETY: OSVERSIONINFOW is plain data; dwOSVersionInfoSize is set as the API requires.
        unsafe {
            let mut info: OSVERSIONINFOW = std::mem::zeroed();
            info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
            if RtlGetVersion(&mut info) == 0 {
                info.dwBuildNumber
            } else {
                0
            }
        }
    }

    fn runtime_version(&self) -> String {
        Self::RUSTC_VERSION.to_string()
    }
}

fn read_registry_string(subkey: &str, value: &str) -> Option<String> {
    let subkey: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let value: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    let mut size: u32 = 0;
    // SAFETY: the names are NUL-terminated; a null buffer asks for the size, then a buffer of
    // that size receives the string.
    unsafe {
        if RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        ) != ERROR_SUCCESS
            || size == 0
        {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        if RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}
