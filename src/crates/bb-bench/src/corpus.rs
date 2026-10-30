//! The cached synthetic corpus: an index database cut from `SyntheticPaths`, and a tree of real files
//! the file-system benchmarks, the harness-less Tier C launches and the hashing runs read. Spec:
//! tools/BertBrowser.Bench/Corpus/BenchCorpus.cs.
//!
//! Everything lives under one folder whose name says what it is (`bertbrowser-bench-native`), and a
//! rebuild only ever deletes a folder carrying this module's marker -- the corpus is never built on,
//! or cleaned out of, a real folder. The database describes drive `Q:`, which does not exist.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use bb_core::data::{Db, DirSizeRepository, FsIndexRepository};
use bb_core::paths::PathKey;

use crate::rng::Rng;
use crate::synthetic::{DRIVE, GENERATOR_VERSION, SyntheticPaths};

pub const TREE_LAYOUT_VERSION: u32 = 2;
pub const NEEDLE: &str = "benchneedle";
const READY: &str = ".ready";
const MEGABYTE: usize = 1024 * 1024;
const UPSERT_CHUNK: usize = 20_000;
/// The folder-name stem every corpus root must carry.
const ROOT_STEM: &str = "bertbrowser-bench";

pub type Log<'a> = &'a dyn Fn(&str);

#[derive(Clone, Debug)]
pub struct TreePaths {
    pub root: PathBuf,
    pub flat: PathBuf,
    pub startup: PathBuf,
    pub deep: PathBuf,
    pub text: PathBuf,
    pub hash: PathBuf,
    pub big: PathBuf,
    pub big_copy: PathBuf,
}

pub fn default_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("bertbrowser-bench-native")
}

/// Refuses a root that is not recognisably a bench corpus, so `--rebuild` can never be pointed at a
/// folder somebody lives in.
pub fn check_root(root: &Path) -> Result<(), String> {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.contains(ROOT_STEM) {
        Ok(())
    } else {
        Err(format!(
            "{} is not a benchmark corpus folder (its name must contain '{ROOT_STEM}').",
            root.display()
        ))
    }
}

pub fn db_path(root: &Path, scale: u32) -> PathBuf {
    root.join(format!("corpus-v{GENERATOR_VERSION}-s{scale}.db"))
}

pub fn tree_root(root: &Path, scale: u32) -> PathBuf {
    root.join(format!(
        "tree-v{GENERATOR_VERSION}.{TREE_LAYOUT_VERSION}-s{scale}"
    ))
}

pub fn tree_paths(root: &Path, scale: u32) -> TreePaths {
    let tree = tree_root(root, scale);
    TreePaths {
        flat: tree.join("Flat"),
        startup: tree.join("Startup"),
        deep: tree.join("Deep"),
        text: tree.join("Text"),
        hash: tree.join("Hash"),
        big: tree.join("Big64MB.bin"),
        big_copy: tree.join("Big64MB-copy.bin"),
        root: tree,
    }
}

pub fn ensure_all(root: &Path, scale: u32, log: Log, rebuild: bool) -> Result<(), String> {
    check_root(root)?;
    ensure_db(root, scale, log, rebuild)?;
    ensure_tree(root, scale, log, rebuild)?;
    Ok(())
}

pub fn ensure_db(root: &Path, scale: u32, log: Log, rebuild: bool) -> Result<PathBuf, String> {
    check_root(root)?;
    let path = db_path(root, scale);
    let marker = marker_of(&path);
    if !rebuild && marker.exists() && path.exists() {
        return Ok(path);
    }

    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    delete_db_files(&path);
    let _ = fs::remove_file(&marker);

    let clock = Instant::now();
    log(&format!(
        "# corpus: generating {} rows (scale {scale})",
        scale as usize * 100_000
    ));
    let model = SyntheticPaths::generate(scale);

    let db = Arc::new(Db::new(&path));
    db.migrate().map_err(|e| e.to_string())?;
    let index = FsIndexRepository::new(db.clone());
    let sizes = DirSizeRepository::new(db.clone());

    let rows: Vec<_> = model.rows().cloned().collect();
    for chunk in rows.chunks(UPSERT_CHUNK) {
        index.upsert_entries(chunk, 1).map_err(|e| e.to_string())?;
    }
    let root_key = PathKey::canonicalize(DRIVE).map_err(|e| e.to_string())?;
    index
        .upsert_root(
            &root_key,
            DRIVE,
            model.directory_sizes[0].computed_utc,
            true,
        )
        .map_err(|e| e.to_string())?;
    sizes
        .upsert_many(model.directory_sizes.iter())
        .map_err(|e| e.to_string())?;

    {
        let conn = db.open().map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| e.to_string())?;
    }
    db.clear_pool();
    drop(db);

    fs::write(&marker, "ready").map_err(|e| e.to_string())?;
    let mb = fs::metadata(&path)
        .map(|m| m.len() / MEGABYTE as u64)
        .unwrap_or(0);
    log(&format!(
        "# corpus: {} ({mb} MB) in {:.1} s",
        path.display(),
        clock.elapsed().as_secs_f64()
    ));
    Ok(path)
}

fn marker_of(db: &Path) -> PathBuf {
    let mut s = db.as_os_str().to_os_string();
    s.push(READY);
    PathBuf::from(s)
}

fn delete_db_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut s = path.as_os_str().to_os_string();
        s.push(suffix);
        let _ = fs::remove_file(PathBuf::from(s));
    }
}

pub fn ensure_tree(root: &Path, scale: u32, log: Log, rebuild: bool) -> Result<TreePaths, String> {
    check_root(root)?;
    let paths = tree_paths(root, scale);
    let marker = paths.root.join(READY);
    if !rebuild && marker.exists() {
        return Ok(paths);
    }

    // The root was just checked to be a bench corpus folder, and the tree folder inside it is ours.
    if paths.root.exists() {
        fs::remove_dir_all(&paths.root).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&paths.root).map_err(|e| e.to_string())?;

    let clock = Instant::now();
    log(&format!(
        "# corpus: writing tree under {} (a few hundred MB; once)",
        paths.root.display()
    ));
    let s = scale as usize;
    let mut rng = Rng::new(777 + u64::from(scale));
    write_flat(&paths.flat, 10_000 * s).map_err(|e| e.to_string())?;
    write_flat(&paths.startup, 500).map_err(|e| e.to_string())?;
    write_deep(&paths.deep, 20_000 * s, 2_000 * s, &mut rng).map_err(|e| e.to_string())?;
    write_text(&paths.text, 2_000 * s, &mut rng).map_err(|e| e.to_string())?;
    write_hash(&paths.hash, &mut rng).map_err(|e| e.to_string())?;
    write_random(&paths.big, 64 * MEGABYTE, &mut rng).map_err(|e| e.to_string())?;
    fs::copy(&paths.big, &paths.big_copy).map_err(|e| e.to_string())?;

    fs::write(&marker, "ready").map_err(|e| e.to_string())?;
    log(&format!(
        "# corpus: tree ready in {:.1} s",
        clock.elapsed().as_secs_f64()
    ));
    Ok(paths)
}

/// One line saying what is in the corpus, for `corpus --stats`.
pub fn describe(root: &Path, scale: u32) -> String {
    let path = db_path(root, scale);
    if !path.exists() {
        return "not built".to_string();
    }
    let db = Db::attach(&path);
    let counts = db.open().ok().and_then(|conn| {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(is_dir), 0), COALESCE(SUM(size_bytes), 0) FROM fs_entry;",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        )
        .ok()
    });
    let tree = tree_root(root, scale);
    let tree_bytes = if tree.exists() { dir_bytes(&tree) } else { 0 };
    let db_mb = fs::metadata(&path)
        .map(|m| m.len() / MEGABYTE as u64)
        .unwrap_or(0);
    match counts {
        Some((rows, dirs, bytes)) => format!(
            "{rows} rows ({dirs} directories), {:.1} GB of invented files, {db_mb} MB database; {} MB of real files in the tree",
            bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            tree_bytes / MEGABYTE as u64
        ),
        None => "database unreadable".to_string(),
    }
}

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = fs::read_dir(&d) else { continue };
        for entry in read.flatten() {
            match entry.metadata() {
                Ok(m) if m.is_dir() => stack.push(entry.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

// ---- the tree ----

const EXTENSION_CYCLE: [&str; 8] = [
    ".jpg", ".png", ".mp4", ".txt", ".cs", ".json", ".xml", ".txt",
];

fn write_flat(dir: &Path, count: usize) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    for i in 0..count {
        File::create(dir.join(format!("f{i:05}{}", EXTENSION_CYCLE[i % 8])))?;
    }
    Ok(())
}

fn write_deep(root: &Path, files: usize, dirs: usize, rng: &mut Rng) -> std::io::Result<()> {
    fs::create_dir_all(root)?;
    let mut folders: Vec<(PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    let mut d = 0;
    while d < dirs {
        let (parent, depth) = folders[rng.below(folders.len())].clone();
        if depth >= 8 {
            continue;
        }
        let path = parent.join(format!("d{d:04}"));
        fs::create_dir(&path)?;
        folders.push((path, depth + 1));
        d += 1;
    }

    let mut buffer = vec![0u8; 4096];
    rng.fill(&mut buffer);
    for i in 0..files {
        let folder = &folders[1 + i % (folders.len() - 1)].0;
        let size = rng.below(4097);
        let mut file = File::create(folder.join(format!("f{i:05}{}", EXTENSION_CYCLE[i % 8])))?;
        file.write_all(&buffer[..size])?;
    }
    Ok(())
}

fn write_text(dir: &Path, count: usize, rng: &mut Rng) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    const WORDS: [&str; 22] = [
        "the", "index", "reads", "every", "name", "once", "and", "patches", "it", "from", "the",
        "journal", "so", "a", "search", "costs", "a", "range", "scan", "never", "a", "walk",
    ];
    for i in 0..count {
        let size = 2 * 1024 + rng.below(62 * 1024);
        let path = dir.join(format!("t{i:05}.txt"));
        if i % 10 == 9 {
            let mut bytes = vec![0u8; size];
            rng.fill(&mut bytes);
            fs::write(path, bytes)?;
            continue;
        }

        let mut text = String::with_capacity(size + 32);
        let mut needle_at = if i % 10 == 0 {
            Some(size / 4 + rng.below(size / 4))
        } else {
            None
        };
        while text.len() < size {
            if needle_at.is_some_and(|at| text.len() >= at) {
                text.push_str(NEEDLE);
                text.push(' ');
                needle_at = None;
            }
            text.push_str(WORDS[rng.below(WORDS.len())]);
            text.push_str(if rng.below(12) == 0 { ".\n" } else { " " });
        }
        fs::write(path, text)?;
    }
    Ok(())
}

fn write_hash(dir: &Path, rng: &mut Rng) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut buffer = vec![0u8; MEGABYTE];
    for i in 0..32 {
        rng.fill(&mut buffer);
        fs::write(dir.join(format!("unique{i:02}.bin")), &buffer)?;
    }
    for i in 0..25 {
        rng.fill(&mut buffer);
        fs::write(dir.join(format!("pair{i:02}-a.bin")), &buffer)?;
        fs::write(dir.join(format!("pair{i:02}-b.bin")), &buffer)?;
    }
    // The same first 64 KB on every one, then different bytes: the prefix pass groups them all and
    // the full pass has to read every byte to tell them apart.
    let mut prefix = vec![0u8; 64 * 1024];
    rng.fill(&mut prefix);
    for i in 0..100 {
        rng.fill(&mut buffer);
        buffer[..prefix.len()].copy_from_slice(&prefix);
        fs::write(dir.join(format!("nearmiss{i:03}.bin")), &buffer)?;
    }
    Ok(())
}

fn write_random(path: &Path, bytes: usize, rng: &mut Rng) -> std::io::Result<()> {
    let mut buffer = vec![0u8; MEGABYTE];
    let mut file = File::create(path)?;
    let mut written = 0;
    while written < bytes {
        rng.fill(&mut buffer);
        let n = buffer.len().min(bytes - written);
        file.write_all(&buffer[..n])?;
        written += n;
    }
    Ok(())
}

// ---- real data ----

/// `bench/real.local.json`: the developer's own index and folders, read-only and never committed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealConfig {
    pub db_path: String,
    pub search_root: String,
    pub listing_folder: String,
    pub tree_folder: String,
    pub text_folder: String,
    pub media_folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_copy_path: Option<String>,
}

pub fn load_real(path: &Path) -> Result<RealConfig, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: RealConfig =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    for (name, value) in [
        ("dbPath", &config.db_path),
        ("searchRoot", &config.search_root),
        ("listingFolder", &config.listing_folder),
        ("treeFolder", &config.tree_folder),
        ("textFolder", &config.text_folder),
        ("mediaFolder", &config.media_folder),
    ] {
        if value.trim().is_empty() {
            return Err(format!(
                "{}: '{name}' is missing. See bench/README.md for the shape.",
                path.display()
            ));
        }
    }
    if !Path::new(&config.db_path).is_file() {
        return Err(format!(
            "{}: no database at {}.",
            path.display(),
            config.db_path
        ));
    }
    Ok(config)
}

/// A copy of the live database, taken once a day with `VACUUM INTO` over a read-only connection:
/// the benchmarks only ever touch the copy, never the database the app and the helper are writing.
pub fn ensure_backup(
    config: &RealConfig,
    cache_root: &Path,
    log: Log,
    refresh: bool,
) -> Result<PathBuf, String> {
    check_root(cache_root)?;
    let dir = cache_root.join("real");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = crate::env::local_date_stamp();
    let copy = dir.join(format!("bertbrowser-{stamp}.db"));
    if copy.exists() && !refresh {
        log(&format!("# real: using today's backup {}", copy.display()));
        return Ok(copy);
    }

    log(&format!(
        "# real: backing up {} -> {}",
        config.db_path,
        copy.display()
    ));
    let clock = Instant::now();
    delete_db_files(&copy);
    let source = rusqlite::Connection::open_with_flags(
        &config.db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    let target = copy.to_string_lossy().replace('\'', "''");
    source
        .execute_batch(&format!("VACUUM INTO '{target}';"))
        .map_err(|e| e.to_string())?;
    let mb = fs::metadata(&copy)
        .map(|m| m.len() / MEGABYTE as u64)
        .unwrap_or(0);
    log(&format!(
        "# real: {mb} MB in {:.1} s",
        clock.elapsed().as_secs_f64()
    ));
    Ok(copy)
}

pub fn write_resolved(config: &RealConfig, cache_root: &Path) -> Result<PathBuf, String> {
    let path = cache_root.join("real").join("resolved.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(path)
}
