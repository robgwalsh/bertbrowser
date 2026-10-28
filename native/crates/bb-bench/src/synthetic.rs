//! One seeded, invented disk: the directory tree, every file's name, size, dates and attributes.
//! Spec: tools/BertBrowser.Bench/Corpus/SyntheticPaths.cs.
//!
//! The index corpus and the directory-size rows are cut from this one model. It lives on drive `Q:`,
//! which does not exist, so nothing here can ever be confused with a real folder. The shape follows
//! the C# generator (a developer's machine: `Users`, `Source`, `Windows`, `Program Files`; most files
//! small; a handful over 100 MB; 1% modified "today", a fixed date so the corpus never ages) but the
//! stream of random numbers is this crate's own, so the rows are the same *kind* of disk rather than
//! the same disk. Same seed and scale, same disk, every time.

use std::collections::{HashMap, HashSet};

use bb_core::FileTime;
use bb_core::data::{DirSizeResult, FsEntryRow};
use bb_core::paths::PathKey;

use crate::rng::Rng;

pub const GENERATOR_VERSION: u32 = 1;
pub const DRIVE: &str = "Q:\\";
pub const SEARCH_ROOT: &str = "Q:\\Source";
/// The benchmark query is `dm:2026-09-15`, never `dm:today`.
pub const TODAY_QUERY: &str = "dm:2026-09-15";

const ATTR_READONLY: u32 = 0x1;
const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;
const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_ARCHIVE: u32 = 0x20;

/// Seconds since 1970 for a civil UTC date (Howard Hinnant's days-from-civil).
const fn unix_secs(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

fn filetime(unix: f64) -> FileTime {
    FileTime(((unix + 11_644_473_600.0) * 10_000_000.0) as i64)
}

const TODAY: i64 = unix_secs(2026, 9, 15);
const DATE_FLOOR: i64 = unix_secs(2021, 10, 1);
const DATE_CEILING: i64 = unix_secs(2026, 9, 30);

pub struct SyntheticPaths {
    pub directories: Vec<FsEntryRow>,
    pub files: Vec<FsEntryRow>,
    /// A `dir_size_cache` row for every directory, drive root included.
    pub directory_sizes: Vec<DirSizeResult>,
}

struct DirNode {
    path: String,
    depth: u32,
    hidden: bool,
    parent: usize,
    bytes: i64,
    file_count: i32,
    dir_count: i32,
}

fn join(parent: &str, name: &str) -> String {
    format!("{}\\{}", parent.trim_end_matches('\\'), name)
}

fn parent_of(path: &str) -> &str {
    let at = path.rfind('\\').unwrap_or(0);
    if at <= 2 { &path[..3] } else { &path[..at] }
}

fn name_of(path: &str) -> &str {
    &path[path.rfind('\\').map_or(0, |i| i + 1)..]
}

fn upper(path: &str) -> String {
    path.to_ascii_uppercase()
}

fn key(path: &str) -> String {
    PathKey::canonicalize(path).expect("a synthetic path canonicalizes")
}

impl SyntheticPaths {
    /// Every row, directories first, each parent before its children.
    pub fn rows(&self) -> impl Iterator<Item = &FsEntryRow> {
        self.directories.iter().chain(self.files.iter())
    }

    pub fn generate(scale: u32) -> SyntheticPaths {
        assert!(scale >= 1);
        let mut rng = Rng::new(12_345 + u64::from(scale));
        let target = scale as usize * 100_000;
        let dir_target = (target / 12).max(SKELETON.len() + 16);
        let file_target = target - dir_target;

        let mut nodes = Vec::with_capacity(dir_target + 1);
        nodes.push(DirNode {
            path: DRIVE.to_string(),
            depth: 0,
            hidden: false,
            parent: usize::MAX,
            bytes: 0,
            file_count: 0,
            dir_count: 0,
        });
        let mut by_path: HashMap<String, usize> = HashMap::new();
        by_path.insert(upper(DRIVE), 0);

        for (relative, hidden) in SKELETON {
            let path = format!("{DRIVE}{relative}");
            let parent = by_path[&upper(parent_of(&path))];
            let node = DirNode {
                depth: nodes[parent].depth + 1,
                hidden: *hidden || nodes[parent].hidden,
                parent,
                path: path.clone(),
                bytes: 0,
                file_count: 0,
                dir_count: 0,
            };
            by_path.insert(upper(&path), nodes.len());
            nodes.push(node);
        }

        while nodes.len() <= dir_target {
            let parent = 1 + rng.below(nodes.len() - 1);
            if nodes[parent].depth >= 14 {
                continue;
            }
            let dotted = rng.next_f64() < 0.015;
            let name = format!("{}{}", if dotted { "." } else { "" }, dir_name(&mut rng));
            let path = join(&nodes[parent].path, &name);
            if by_path.contains_key(&upper(&path)) {
                continue;
            }
            by_path.insert(upper(&path), nodes.len());
            let node = DirNode {
                depth: nodes[parent].depth + 1,
                hidden: dotted || nodes[parent].hidden,
                parent,
                path,
                bytes: 0,
                file_count: 0,
                dir_count: 0,
            };
            nodes.push(node);
        }

        let mut files = Vec::with_capacity(file_target);
        let mut used: HashSet<String> = HashSet::with_capacity(file_target);
        while files.len() < file_target {
            let parent_index = 1 + rng.below(nodes.len() - 1);
            let name = file_name(&mut rng);
            let path = join(&nodes[parent_index].path, &name);
            if !used.insert(upper(&path)) {
                continue;
            }

            let size = file_size(&mut rng);
            let modified = modified(&mut rng);
            let created = if rng.next_f64() < 0.05 {
                modified + (rng.range(1, 400) * 86_400) as f64
            } else {
                modified
            };
            let hidden = nodes[parent_index].hidden || rng.next_f64() < 0.03;
            let mut attributes = ATTR_ARCHIVE;
            if hidden {
                attributes |= ATTR_HIDDEN;
            }
            if rng.next_f64() < 0.02 {
                attributes |= ATTR_READONLY;
            }
            if rng.next_f64() < 0.005 {
                attributes |= ATTR_SYSTEM;
            }

            files.push(FsEntryRow {
                path_key: key(&path),
                name,
                is_directory: false,
                size_bytes: size,
                modified_utc: filetime(modified),
                hidden,
                attributes,
                created_utc: filetime(created),
            });
            nodes[parent_index].bytes += size;
            nodes[parent_index].file_count += 1;
        }

        // Children always follow their parents, so one reverse pass is a complete post-order fold.
        for i in (1..nodes.len()).rev() {
            let (bytes, files_n, dirs_n, parent) = {
                let n = &nodes[i];
                (n.bytes, n.file_count, n.dir_count, n.parent)
            };
            let p = &mut nodes[parent];
            p.bytes += bytes;
            p.file_count += files_n;
            p.dir_count += dirs_n + 1;
        }

        let mut directories = Vec::with_capacity(nodes.len() - 1);
        let mut directory_sizes = Vec::with_capacity(nodes.len());
        let computed = filetime((TODAY + 3_600) as f64);
        for node in &nodes {
            directory_sizes.push(DirSizeResult {
                path_key: key(&node.path),
                size_bytes: node.bytes,
                file_count: node.file_count,
                dir_count: node.dir_count,
                incomplete: false,
                computed_utc: computed,
            });
            if node.depth == 0 {
                continue;
            }
            let modified = filetime(modified_secs(&mut rng));
            directories.push(FsEntryRow {
                path_key: key(&node.path),
                name: name_of(&node.path).to_string(),
                is_directory: true,
                size_bytes: 0,
                modified_utc: modified,
                hidden: node.hidden,
                attributes: ATTR_DIRECTORY | if node.hidden { ATTR_HIDDEN } else { 0 },
                created_utc: modified,
            });
        }

        SyntheticPaths {
            directories,
            files,
            directory_sizes,
        }
    }
}

fn modified_secs(rng: &mut Rng) -> f64 {
    if rng.next_f64() < 0.01 {
        return (TODAY + rng.range(0, 86_400)) as f64;
    }
    DATE_FLOOR as f64 + rng.next_f64() * (DATE_CEILING - DATE_FLOOR) as f64
}

/// Seconds since 1970 (as the model's rows hold them only through `filetime`).
fn modified(rng: &mut Rng) -> f64 {
    modified_secs(rng)
}

fn dir_name(rng: &mut Rng) -> String {
    let word = WORDS[rng.below(WORDS.len())];
    match rng.below(4) {
        0 => format!("{word}-{}", WORDS[rng.below(WORDS.len())]),
        1 => format!("{word}{}", rng.range(1, 40)),
        _ => word.to_string(),
    }
}

fn file_name(rng: &mut Rng) -> String {
    let roll = rng.next_f64();
    if roll < 0.02 {
        return format!("IMG_{:04}.jpg", rng.range(0, 10_000));
    }
    if roll < 0.023 {
        return format!("report{}", extension(rng));
    }
    if roll < 0.028 {
        let word = WORDS[rng.below(WORDS.len())];
        return format!("{word}-report-{}{}", rng.range(2019, 2027), extension(rng));
    }

    let mut name = WORDS[rng.below(WORDS.len())].to_string();
    match rng.below(5) {
        0 => {
            name.push('-');
            name.push_str(WORDS[rng.below(WORDS.len())]);
        }
        1 => name.push_str(&format!("_{:04}", rng.range(0, 10_000))),
        2 => {
            let mut chars = name.chars();
            let first = chars
                .next()
                .expect("words are not empty")
                .to_ascii_uppercase();
            name = format!("{first}{}{}", chars.as_str(), WORDS[rng.below(WORDS.len())]);
        }
        _ => {}
    }
    name.push_str(extension(rng));
    name
}

fn extension(rng: &mut Rng) -> &'static str {
    let total: f64 = EXTENSIONS.iter().map(|e| e.1).sum();
    let mut roll = rng.next_f64() * total;
    for (ext, weight) in EXTENSIONS {
        roll -= weight;
        if roll <= 0.0 {
            return ext;
        }
    }
    EXTENSIONS[EXTENSIONS.len() - 1].0
}

fn file_size(rng: &mut Rng) -> i64 {
    if rng.next_f64() < 0.006 {
        return rng.range(100 * 1024 * 1024, 1024 * 1024 * 1024);
    }
    // Log-normal, median 6 KB, long right tail; capped below the 100 GB the worst-case query asks for.
    let gaussian = (-2.0 * (1.0 - rng.next_f64()).ln()).sqrt()
        * (2.0 * std::f64::consts::PI * rng.next_f64()).cos();
    let size = (8.72 + 2.6 * gaussian).exp();
    size.clamp(0.0, 50.0 * 1024.0 * 1024.0 * 1024.0) as i64
}

const SKELETON: &[(&str, bool)] = &[
    ("Users", false),
    ("Users\\bench", false),
    ("Users\\bench\\Documents", false),
    ("Users\\bench\\Pictures", false),
    ("Users\\bench\\Videos", false),
    ("Users\\bench\\Downloads", false),
    ("Users\\bench\\Desktop", false),
    ("Users\\bench\\Music", false),
    ("Users\\bench\\AppData", true),
    ("Users\\bench\\AppData\\Local", false),
    ("Users\\bench\\AppData\\Roaming", false),
    ("Users\\bench\\AppData\\Local\\Temp", false),
    ("Users\\bench\\AppData\\Local\\Packages", false),
    ("Users\\Public", false),
    ("Users\\Default", true),
    ("Source", false),
    ("Source\\bertbrowser", false),
    ("Source\\bertbrowser\\src", false),
    ("Source\\bertbrowser\\tests", false),
    ("Source\\ledger", false),
    ("Source\\ledger\\src", false),
    ("Source\\ledger\\node_modules", false),
    ("Source\\website", false),
    ("Source\\website\\node_modules", false),
    ("Source\\website\\public", false),
    ("Source\\kernel-tools", false),
    ("Source\\kernel-tools\\build", false),
    ("Source\\photoshelf", false),
    ("Source\\photoshelf\\bin", false),
    ("Source\\infra", false),
    ("Source\\infra\\terraform", false),
    ("Source\\notes", false),
    ("Source\\scratch", false),
    ("Source\\playground", false),
    ("Source\\vendor", false),
    ("Source\\vendor\\sdk", false),
    ("Source\\archive", false),
    ("Source\\archive\\2019", false),
    ("Source\\archive\\2021", false),
    ("Windows", false),
    ("Windows\\System32", false),
    ("Windows\\System32\\drivers", false),
    ("Windows\\System32\\DriverStore", false),
    ("Windows\\SysWOW64", false),
    ("Windows\\WinSxS", false),
    ("Windows\\WinSxS\\Manifests", false),
    ("Windows\\Temp", false),
    ("Windows\\assembly", false),
    ("Windows\\Fonts", false),
    ("Windows\\Logs", false),
    ("Windows\\servicing", false),
    ("Program Files", false),
    ("Program Files\\Common Files", false),
    ("Program Files\\dotnet", false),
    ("Program Files\\dotnet\\shared", false),
    ("Program Files\\Git", false),
    ("Program Files\\Git\\mingw64", false),
    ("Program Files\\7-Zip", false),
    ("Program Files\\Microsoft VS Code", false),
    ("Program Files\\Microsoft VS Code\\resources", false),
    ("Program Files\\Mozilla Firefox", false),
    ("Program Files\\Google", false),
    ("Program Files\\Google\\Chrome", false),
    ("Program Files\\Microsoft Office", false),
    ("Program Files\\Microsoft Office\\root", false),
    ("Program Files\\Windows Defender", false),
    ("Program Files\\WindowsApps", true),
    ("Program Files\\Adobe", false),
    ("Program Files\\Blender Foundation", false),
    ("Program Files\\Docker", false),
    ("Program Files\\Java", false),
    ("Program Files\\Python312", false),
    ("Program Files\\Python312\\Lib", false),
    ("Program Files\\NVIDIA Corporation", false),
    ("Program Files\\Steam", false),
    ("Program Files\\Steam\\steamapps", false),
    ("Program Files\\OBS Studio", false),
    ("Program Files\\VideoLAN", false),
    ("Program Files\\PowerShell", false),
    ("Program Files\\Unity", false),
    ("Program Files\\Unity\\Editor", false),
    ("Program Files\\JetBrains", false),
    ("Program Files\\Notepad++", false),
    ("Program Files\\PuTTY", false),
    ("Program Files\\WinSCP", false),
    ("Program Files (x86)", false),
    ("Program Files (x86)\\Common Files", false),
    ("Program Files (x86)\\Microsoft SDKs", false),
    ("Program Files (x86)\\Windows Kits", false),
    ("Program Files (x86)\\Windows Kits\\10", false),
    ("Program Files (x86)\\Steam", false),
    ("Program Files (x86)\\Internet Explorer", false),
    ("Program Files (x86)\\Microsoft", false),
    ("Program Files (x86)\\Reference Assemblies", false),
    ("Program Files (x86)\\MSBuild", false),
    ("Program Files (x86)\\Intel", false),
    ("ProgramData", true),
    ("ProgramData\\Microsoft", false),
    ("ProgramData\\Package Cache", false),
    ("ProgramData\\Docker", false),
    ("ProgramData\\chocolatey", false),
    ("$Recycle.Bin", true),
    ("Recovery", true),
    ("PerfLogs", false),
    ("inetpub", false),
];

const EXTENSIONS: &[(&str, f64)] = &[
    (".dll", 14.0),
    (".txt", 9.0),
    (".cs", 8.0),
    (".json", 7.0),
    (".xml", 6.0),
    (".png", 6.0),
    (".jpg", 6.0),
    (".h", 4.0),
    (".cpp", 4.0),
    (".js", 4.0),
    (".ts", 3.0),
    (".md", 3.0),
    (".log", 3.0),
    (".exe", 2.0),
    (".mp4", 1.5),
    (".pdf", 1.5),
    (".zip", 1.0),
    (".manifest", 1.5),
    (".mui", 1.5),
    (".cat", 1.0),
    (".pri", 1.0),
    (".dat", 1.0),
    (".svg", 0.5),
    (".css", 0.5),
    (".html", 0.5),
    (".yml", 0.5),
    (".docx", 0.3),
    (".xlsx", 0.3),
    (".mp3", 0.3),
    (".gif", 0.3),
    (".ico", 0.3),
    (".nupkg", 0.3),
    (".pdb", 1.0),
    (".lib", 0.5),
    (".obj", 0.5),
    (".resx", 0.3),
    (".xaml", 0.3),
    (".ps1", 0.2),
];

const WORDS: &[&str] = &[
    "alpha",
    "anchor",
    "annual",
    "api",
    "app",
    "archive",
    "asset",
    "audio",
    "backup",
    "banner",
    "base",
    "batch",
    "bench",
    "binary",
    "block",
    "board",
    "bridge",
    "buffer",
    "build",
    "bundle",
    "cache",
    "camera",
    "canvas",
    "card",
    "catalog",
    "chain",
    "channel",
    "chart",
    "client",
    "cloud",
    "cluster",
    "code",
    "color",
    "common",
    "compat",
    "config",
    "console",
    "content",
    "control",
    "copy",
    "core",
    "crypto",
    "cursor",
    "daemon",
    "data",
    "debug",
    "default",
    "demo",
    "design",
    "desktop",
    "device",
    "dialog",
    "diff",
    "digest",
    "disk",
    "display",
    "doc",
    "draft",
    "driver",
    "editor",
    "engine",
    "entry",
    "error",
    "event",
    "export",
    "extension",
    "feature",
    "field",
    "file",
    "filter",
    "final",
    "folder",
    "font",
    "form",
    "frame",
    "game",
    "gateway",
    "graph",
    "grid",
    "group",
    "guide",
    "handler",
    "header",
    "helper",
    "history",
    "home",
    "host",
    "icon",
    "image",
    "import",
    "index",
    "info",
    "input",
    "install",
    "invoice",
    "item",
    "job",
    "journal",
    "kernel",
    "key",
    "label",
    "layer",
    "layout",
    "ledger",
    "legacy",
    "library",
    "license",
    "link",
    "list",
    "loader",
    "local",
    "log",
    "machine",
    "main",
    "manifest",
    "map",
    "media",
    "memo",
    "menu",
    "merge",
    "message",
    "meta",
    "model",
    "module",
    "monitor",
    "mount",
    "native",
    "network",
    "node",
    "notes",
    "object",
    "offline",
    "output",
    "package",
    "page",
    "panel",
    "parser",
    "patch",
    "path",
    "photo",
    "pipeline",
    "plan",
    "player",
    "plugin",
    "policy",
    "pool",
    "preview",
    "profile",
    "project",
    "proxy",
    "query",
    "queue",
    "readme",
    "record",
    "registry",
    "release",
    "render",
    "resource",
    "result",
    "review",
    "root",
    "route",
    "runtime",
    "sample",
    "scan",
    "schema",
    "screen",
    "script",
    "search",
    "secret",
    "segment",
    "server",
    "service",
    "session",
    "settings",
    "setup",
    "shader",
    "shared",
    "shell",
    "signal",
    "sketch",
    "slide",
    "snapshot",
    "socket",
    "source",
    "spec",
    "stack",
    "stage",
    "state",
    "status",
    "storage",
    "store",
    "stream",
    "string",
    "style",
    "summary",
    "support",
    "sync",
    "system",
    "table",
    "target",
    "task",
    "template",
    "temp",
    "test",
    "texture",
    "theme",
    "thread",
    "thumbnail",
    "ticket",
    "timeline",
    "token",
    "tool",
    "trace",
    "track",
    "transfer",
    "tree",
    "update",
    "upload",
    "user",
    "util",
    "value",
    "vendor",
    "version",
    "video",
    "view",
    "volume",
    "wallet",
    "web",
    "widget",
    "window",
    "worker",
    "workspace",
    "zone",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_one_disk_has_the_documented_shape() {
        let model = SyntheticPaths::generate(1);
        assert_eq!(model.directories.len() + model.files.len(), 100_000);
        assert_eq!(SyntheticPaths::generate(1).files[1234], model.files[1234]);
        assert!(model.files.iter().any(|f| f.size_bytes > 100 * 1024 * 1024));
        assert!(
            model
                .files
                .iter()
                .all(|f| f.size_bytes < 100 * 1024 * 1024 * 1024)
        );
        assert!(model.files.iter().any(|f| f.name.starts_with("IMG_")));
        assert!(model.directories[0].path_key.starts_with("Q:\\"));
        assert_eq!(model.directory_sizes.len(), model.directories.len() + 1);
    }

    #[test]
    fn today_is_the_fixed_date() {
        assert_eq!(TODAY, 1_789_430_400);
    }
}
