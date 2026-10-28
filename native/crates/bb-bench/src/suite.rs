//! The Tier A benchmarks: bb-core's hot paths, under the ids the C# suite uses
//! (`core.<class>.<method>[(param=value,...)]`) wherever the code exists natively. Spec:
//! tools/BertBrowser.Bench/Benchmarks/*.cs.
//!
//! Not every C# suite has a native counterpart yet (the archive index, the MFT builder, the change
//! log, folder comparison, content scanning): a benchmark is added here when the code it measures
//! is in bb-core, and an id missing from a native baseline is simply not compared.

use std::cell::RefCell;
use std::hint::black_box;
use std::io::Cursor;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use bb_core::FileTime;
use bb_core::cancel::CancelToken;
use bb_core::checksums::{ChecksumAlgorithm, FileDigester, FileSystemFileHasher};
use bb_core::data::{Db, DirSizeRepository, FsIndexRepository};
use bb_core::delete::{DeleteMode, DeletePlanner, DeleteSource};
use bb_core::duplicates::{DuplicateScanRequest, FileHasher};
use bb_core::fs::FileSystemService;
use bb_core::fs::{FileListDiff, LocalFileSystem};
use bb_core::models::FileEntry;
use bb_core::paths::PathKey;
use bb_core::preview::animation::{GifDisposal, GifFrameCompositor, GifSubFrame};
use bb_core::preview::{
    HexPreviewReader, SyntaxLanguage, SyntaxTokenizer, TextPreviewReader, TextReadOptions,
};
use bb_core::rename::{RenamePlanner, RenameRule, RenameSource};
use bb_core::search::{GlobText, SearchCandidate, SearchGrammar};
use bb_core::transfer::{TransferMergeExpander, TransferPlanner, TransferVerb};

use crate::corpus::{self, RealConfig, TreePaths};
use crate::fake_disk::FakeDisk;
use crate::rng::Rng;
use crate::runner::{Op, Suite, bench_id};
use crate::synthetic::{SEARCH_ROOT, SyntheticPaths, TODAY_QUERY};

const MEGABYTE: usize = 1024 * 1024;

/// Keeps a result alive without tripping `must_use` on the `Result`s the repositories return.
fn sink<T>(value: T) {
    drop(black_box(value));
}

/// What a run is cut from: the scale, the corpus folder, and the real-data config if `--real`.
pub struct Ctx {
    pub scale: u32,
    pub corpus_root: PathBuf,
    pub real: Option<RealConfig>,
    model: RefCell<Option<Rc<SyntheticPaths>>>,
}

impl Ctx {
    pub fn new(scale: u32, corpus_root: PathBuf, real: Option<RealConfig>) -> Rc<Ctx> {
        Rc::new(Ctx {
            scale,
            corpus_root,
            real,
            model: RefCell::new(None),
        })
    }

    fn model(&self) -> Rc<SyntheticPaths> {
        self.model
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(SyntheticPaths::generate(self.scale)))
            .clone()
    }

    fn index_db(&self) -> PathBuf {
        match self.real.as_ref().and_then(|r| r.db_copy_path.clone()) {
            Some(p) => PathBuf::from(p),
            None => corpus::db_path(&self.corpus_root, self.scale),
        }
    }

    fn search_root(&self) -> String {
        self.real
            .as_ref()
            .map_or_else(|| SEARCH_ROOT.to_string(), |r| r.search_root.clone())
    }

    fn tree(&self) -> TreePaths {
        corpus::tree_paths(&self.corpus_root, self.scale)
    }

    fn open_index(&self) -> Result<Arc<Db>, String> {
        let path = self.index_db();
        if !path.exists() {
            return Err(format!(
                "no corpus database at {}; run `corpus` first.",
                path.display()
            ));
        }
        Ok(Arc::new(Db::attach(path)))
    }
}

const QUERIES: [&str; 7] = [
    "report",
    "ext:txt",
    TODAY_QUERY,
    "size:>100mb",
    "size:>100gb",
    "report ext:cs size:>1kb",
    "re:^img_",
];

const GRAMMAR_QUERIES: [&str; 9] = [
    "report",
    "ext:txt",
    TODAY_QUERY,
    "size:>100mb",
    "size:>100gb",
    "report ext:cs size:>1kb",
    "re:^img_",
    "content:todo OR ext:md",
    "report OR draft OR final ext:docx;pdf;txt size:>1kb !archive dm:2024 path:source is:file",
];

pub fn register(suite: &mut Suite, ctx: &Rc<Ctx>) {
    paths(suite, ctx);
    search_grammar(suite, ctx);
    index(suite, ctx);
    files(suite, ctx);
    preview(suite);
    planners(suite);
    gif(suite);
}

// ---- PathKey, glob ----

fn paths(suite: &mut Suite, ctx: &Rc<Ctx>) {
    const SHORT: &str = r"C:\Users\bench\notes.txt";
    const DEEP: &str =
        r"C:\Source\bertbrowser\src\BertBrowser.Core\Services\Transfer\TransferMergeExpander.cs";
    const TRAILING: &str = r"C:\Source\bertbrowser\src\BertBrowser.Core\Services\Transfer\";
    const DEEP_DIR: &str = r"C:\SOURCE\BERTBROWSER\SRC\BERTBROWSER.CORE\SERVICES";

    suite.add(
        bench_id("pathkey", "canonicalizeshort", &[]),
        &[],
        0.0,
        || {
            Ok(Op::new(|| {
                drop(black_box(PathKey::canonicalize(black_box(SHORT))))
            }))
        },
    );
    suite.add(
        bench_id("pathkey", "canonicalizedeep", &[]),
        &[],
        0.0,
        || {
            Ok(Op::new(|| {
                drop(black_box(PathKey::canonicalize(black_box(DEEP))))
            }))
        },
    );
    suite.add(
        bench_id("pathkey", "canonicalizetrailing", &[]),
        &[],
        0.0,
        || {
            Ok(Op::new(|| {
                drop(black_box(PathKey::canonicalize(black_box(TRAILING))))
            }))
        },
    );
    suite.add(bench_id("pathkey", "prefixbounds", &[]), &[], 0.0, || {
        Ok(Op::new(|| {
            drop(black_box(PathKey::prefix_bounds(black_box(DEEP_DIR))))
        }))
    });

    // 1,000 file keys and 50 directory keys from the corpus: the shape of a transfer planner asking
    // "is any of these inside any of those", which is O(keys x dirs) calls.
    let keys_and_dirs = |ctx: Rc<Ctx>| {
        move || -> (Vec<String>, Vec<String>) {
            let model = ctx.model();
            let keys = model
                .files
                .iter()
                .take(1000)
                .map(|f| f.path_key.clone())
                .collect();
            let dirs = model
                .directories
                .iter()
                .filter(|d| d.path_key.matches('\\').count() >= 3)
                .take(50)
                .map(|d| d.path_key.clone())
                .collect();
            (keys, dirs)
        }
    };
    let setup = keys_and_dirs(ctx.clone());
    suite.add(
        bench_id("pathkey", "isunderloop", &[]),
        &[],
        0.0,
        move || {
            let (keys, dirs) = setup();
            Ok(Op::new(move || {
                let mut hits = 0;
                for key in &keys {
                    for dir in &dirs {
                        if PathKey::is_under(key, dir).unwrap_or(false) {
                            hits += 1;
                        }
                    }
                }
                black_box(hits);
            }))
        },
    );
    let setup = keys_and_dirs(ctx.clone());
    suite.add(
        bench_id("pathkey", "isunderkeyloop", &[]),
        &[],
        0.0,
        move || {
            let (keys, dirs) = setup();
            Ok(Op::new(move || {
                let mut hits = 0;
                for key in &keys {
                    for dir in &dirs {
                        if PathKey::is_under_key(key, dir) {
                            hits += 1;
                        }
                    }
                }
                black_box(hits);
            }))
        },
    );

    let ctx = ctx.clone();
    suite.add(
        bench_id("glob", "wildcardmatch10k", &[]),
        &[],
        0.0,
        move || {
            const PATTERN: &str = "*REP*ORT?.TXT";
            let names: Vec<String> = ctx
                .model()
                .files
                .iter()
                .take(10_000)
                .map(|f| f.name.to_uppercase())
                .collect();
            Ok(Op::new(move || {
                let mut hits = 0;
                for name in &names {
                    if GlobText::wildcard_match(name, PATTERN) {
                        hits += 1;
                    }
                }
                black_box(hits);
            }))
        },
    );
}

// ---- the search grammar ----

struct Candidates {
    names: Vec<String>,
    model: Rc<SyntheticPaths>,
}

impl Candidates {
    fn new(model: Rc<SyntheticPaths>) -> Candidates {
        let names = model
            .files
            .iter()
            .take(10_000)
            .map(|f| f.name.to_uppercase())
            .collect();
        Candidates { names, model }
    }

    fn each(&self, mut f: impl FnMut(&SearchCandidate<'_>)) {
        for (row, name) in self.model.files.iter().zip(&self.names) {
            f(&SearchCandidate {
                name_key: name,
                path_key: &row.path_key,
                is_directory: row.is_directory,
                size_bytes: row.size_bytes,
                modified_utc: row.modified_utc,
                hidden: row.hidden,
                attributes: row.attributes,
                created_utc: row.created_utc,
                content: None,
            });
        }
    }
}

fn search_grammar(suite: &mut Suite, ctx: &Rc<Ctx>) {
    for query in GRAMMAR_QUERIES {
        let params = [("Query", query)];
        let q = query.to_string();
        suite.add(
            bench_id("searchgrammar", "parse", &params),
            &params,
            0.0,
            move || {
                Ok(Op::new(move || {
                    black_box(SearchGrammar::parse(black_box(&q), false));
                }))
            },
        );
        let q = query.to_string();
        suite.add(
            bench_id("searchgrammar", "compile", &params),
            &params,
            0.0,
            move || {
                let parsed = SearchGrammar::parse(&q, false)
                    .query
                    .ok_or_else(|| format!("'{q}' did not parse."))?;
                Ok(Op::new(move || {
                    black_box(parsed.compile());
                }))
            },
        );
        let q = query.to_string();
        let ctx = ctx.clone();
        suite.add(
            bench_id("searchgrammar", "matches10k", &params),
            &params,
            0.0,
            move || {
                let parsed = SearchGrammar::parse(&q, false)
                    .query
                    .ok_or_else(|| format!("'{q}' did not parse."))?;
                let candidates = Candidates::new(ctx.model());
                Ok(Op::new(move || {
                    let mut hits = 0;
                    candidates.each(|c| {
                        if parsed.matches(c) {
                            hits += 1;
                        }
                    });
                    black_box(hits);
                }))
            },
        );
    }
}

// ---- the index database ----

fn index(suite: &mut Suite, ctx: &Rc<Ctx>) {
    const CAP: usize = 1000;
    for query in QUERIES {
        for global in [false, true] {
            let params = [("Query", query)];
            let method = if global { "searchglobal" } else { "search" };
            let (q, ctx) = (query.to_string(), ctx.clone());
            suite.add(
                bench_id("fsindex", method, &params),
                &params,
                2.0,
                move || {
                    let repo = FsIndexRepository::new(ctx.open_index()?);
                    let root = ctx.search_root();
                    let text = if ctx.real.is_some() && q == TODAY_QUERY {
                        "dm:today"
                    } else {
                        &q
                    };
                    let parsed = SearchGrammar::parse(text, false)
                        .query
                        .ok_or_else(|| format!("'{text}' did not parse."))?;
                    let run = move || {
                        let hits = if global {
                            repo.search_global(&parsed, CAP, false)
                        } else {
                            repo.search(&root, &parsed, CAP, false)
                        };
                        black_box(hits.map(|h| h.0.len()).unwrap_or(0));
                    };
                    run();
                    Ok(Op::new(run))
                },
            );
        }
    }

    for method in [
        "subtree",
        "duplicatecandidates",
        "findcoveringroot",
        "hassizedata",
    ] {
        let ctx = ctx.clone();
        suite.add(bench_id("fsindexscan", method, &[]), &[], 2.0, move || {
            let repo = FsIndexRepository::new(ctx.open_index()?);
            let root = ctx.search_root();
            let cancel = CancelToken::new();
            let _ = repo.find_covering_root(&root);
            Ok(match method {
                "subtree" => Op::new(move || {
                    sink(
                        repo.subtree(&root, false, 500_000, None, &cancel)
                            .map(|r| r.0.len()),
                    );
                }),
                "duplicatecandidates" => Op::new(move || {
                    sink(
                        repo.duplicate_candidates(
                            Some(&root),
                            DuplicateScanRequest::DEFAULT_MIN_SIZE_BYTES,
                            false,
                            None,
                            &cancel,
                        )
                        .map(|s| s.files.len()),
                    );
                }),
                "findcoveringroot" => Op::new(move || {
                    sink(repo.find_covering_root(&root));
                }),
                _ => Op::new(move || {
                    sink(repo.has_size_data(Some(&root)));
                }),
            })
        });
    }

    for n in [50usize, 2000] {
        let ctx = ctx.clone();
        let n_text = n.to_string();
        suite.add(
            bench_id("dirsize", "getmany", &[("N", &n_text)]),
            &[("N", &n_text)],
            2.0,
            move || {
                let db = ctx.open_index()?;
                let repo = DirSizeRepository::new(db.clone());
                let keys: Vec<String> = {
                    let conn = db.open().map_err(|e| e.to_string())?;
                    let mut stmt = conn
                        .prepare("SELECT path_key FROM dir_size_cache ORDER BY path_key LIMIT ?1;")
                        .map_err(|e| e.to_string())?;
                    let rows = stmt
                        .query_map([n as i64], |r| r.get::<_, String>(0))
                        .map_err(|e| e.to_string())?;
                    rows.filter_map(Result::ok).collect()
                };
                Ok(Op::new(move || {
                    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
                    sink(repo.get_many(&refs).map(|m| m.len()));
                }))
            },
        );
    }

    let c = ctx.clone();
    suite.add(
        bench_id("dirsizewrite", "upsertmany10k", &[]),
        &[],
        2.0,
        move || {
            let path = scratch_db();
            let db = Arc::new(Db::new(&path));
            db.migrate().map_err(|e| e.to_string())?;
            let repo = DirSizeRepository::new(db.clone());
            let chunk: Vec<_> = c
                .model()
                .directory_sizes
                .iter()
                .take(10_000)
                .cloned()
                .collect();
            let emptier = db.clone();
            Ok(Op::new(move || {
                black_box(repo.upsert_many(chunk.iter()).is_ok());
            })
            .with_before(move || {
                if let Ok(conn) = emptier.open() {
                    let _ = conn.execute("DELETE FROM dir_size_cache;", []);
                }
            }))
        },
    );

    let c = ctx.clone();
    suite.add(
        bench_id("fsindexwrite", "upsert20k", &[]),
        &[],
        2.0,
        move || {
            let path = scratch_db();
            let db = Arc::new(Db::new(&path));
            db.migrate().map_err(|e| e.to_string())?;
            let repo = FsIndexRepository::new(db.clone());
            let rows: Vec<_> = c.model().files.iter().take(20_000).cloned().collect();
            let mut generation = 0;
            let emptier = db.clone();
            Ok(Op::new(move || {
                generation += 1;
                black_box(repo.upsert_entries(&rows, generation).is_ok());
            })
            .with_before(move || {
                if let Ok(conn) = emptier.open() {
                    let _ = conn.execute("DELETE FROM fs_entry;", []);
                }
            }))
        },
    );

    let c = ctx.clone();
    suite.add(bench_id("db", "open", &[]), &[], 2.0, move || {
        let db = c.open_index()?;
        drop(db.open().map_err(|e| e.to_string())?);
        Ok(Op::new(move || {
            black_box(db.open().is_ok());
        }))
    });
    let c = ctx.clone();
    suite.add(
        bench_id("db", "openwithoutpragmas", &[]),
        &[],
        2.0,
        move || {
            let path = c.index_db();
            let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
            drop(rusqlite::Connection::open_with_flags(&path, flags).map_err(|e| e.to_string())?);
            Ok(Op::new(move || {
                black_box(rusqlite::Connection::open_with_flags(&path, flags).is_ok());
            }))
        },
    );
}

fn scratch_db() -> PathBuf {
    let dir = std::env::temp_dir().join("bertbrowser-bench-native-scratch");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!(
        "scratch-{}-{}.db",
        std::process::id(),
        Rng::new(std::process::id().into()).next_u64()
    ))
}

// ---- files, digests, the file list ----

fn files(suite: &mut Suite, ctx: &Rc<Ctx>) {
    // A digest over the 64 MB file: the sinks are crate-private, so the public path is measured --
    // open, read in 1 MB chunks, hash -- from a warm page cache.
    for algorithm in [
        ChecksumAlgorithm::Crc32,
        ChecksumAlgorithm::Md5,
        ChecksumAlgorithm::Sha1,
        ChecksumAlgorithm::Sha256,
        ChecksumAlgorithm::Sha512,
    ] {
        let name = format!("{algorithm:?}");
        let params = [("Algorithm", name.as_str())];
        let ctx = ctx.clone();
        suite.add(
            bench_id("digest", "append64mb", &params),
            &params,
            0.0,
            move || {
                let big = ctx.tree().big;
                let hasher = FileSystemFileHasher::new();
                let cancel = CancelToken::new();
                let path = big.to_string_lossy().to_string();
                if !big.exists() {
                    return Err(format!("{path} is missing; run `corpus` first."));
                }
                Ok(Op::new(move || {
                    black_box(hasher.digest(&path, &[algorithm], None, &cancel).ok());
                }))
            },
        );
    }

    let c = ctx.clone();
    suite.add(bench_id("hash", "hashfile64mb", &[]), &[], 2.0, move || {
        let path = c.tree().big.to_string_lossy().to_string();
        let hasher = FileSystemFileHasher::new();
        let cancel = CancelToken::new();
        black_box(hasher.hash(&path, i64::MAX, None, &cancel).ok());
        Ok(Op::new(move || {
            black_box(hasher.hash(&path, i64::MAX, None, &cancel).ok());
        }))
    });
    let c = ctx.clone();
    suite.add(
        bench_id("hash", "hashprefix64k", &[]),
        &[],
        2.0,
        move || {
            const PREFIX: i64 = 64 * 1024;
            let dir = c.tree().hash;
            let mut files: Vec<String> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("nearmiss"))
                })
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            files.sort();
            let hasher = FileSystemFileHasher::new();
            let cancel = CancelToken::new();
            Ok(Op::new(move || {
                let mut hashed = 0;
                for file in &files {
                    if hasher
                        .hash(file, PREFIX, None, &cancel)
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        hashed += 1;
                    }
                }
                black_box(hashed);
            }))
        },
    );

    let c = ctx.clone();
    suite.add(
        bench_id("filesystem", "listdirectory", &[]),
        &[],
        2.0,
        move || {
            let folder = c.real.as_ref().map_or_else(
                || c.tree().flat.to_string_lossy().to_string(),
                |r| r.listing_folder.clone(),
            );
            let fs = LocalFileSystem;
            fs.list_directory(&folder)
                .map_err(|e| format!("{folder}: {e}"))?;
            Ok(Op::new(move || {
                sink(fs.list_directory(&folder).map(|l| l.len()));
            }))
        },
    );

    // 10,000 rows; drop the first hundred, change the size of the next hundred, add a hundred new.
    suite.add(
        bench_id("filelistdiff", "compute10k", &[]),
        &[],
        0.0,
        || {
            const ROWS: usize = 10_000;
            const CHURN: usize = ROWS / 100;
            let stamp = FileTime(134_024_160_000_000_000);
            let entry = |i: usize, size: i64| FileEntry {
                name: format!("f{i:05}.txt"),
                full_path: format!(r"C:\Bench\Flat\f{i:05}.txt"),
                is_directory: false,
                size_bytes: size,
                modified_utc: stamp,
                attributes: 0x20,
                created_utc: stamp,
                accessed_utc: stamp,
            };
            let current: Vec<FileEntry> = (0..ROWS).map(|i| entry(i, 64)).collect();
            let mut next: Vec<FileEntry> = (CHURN..ROWS)
                .map(|i| entry(i, if i < 2 * CHURN { 128 } else { 64 }))
                .collect();
            next.extend((ROWS..ROWS + CHURN).map(|i| entry(i, 64)));
            Ok(Op::new(move || {
                black_box(FileListDiff::compute(&current, &next).is_ok());
            }))
        },
    );
}

// ---- the preview readers ----

fn prose(bytes: usize) -> String {
    const SENTENCE: &str = "The index is one table, clustered by path, patched in place by the journal; a search is a range scan.\n";
    let mut text = String::with_capacity(bytes + SENTENCE.len());
    let mut line = 0;
    while text.len() < bytes {
        if line % 7 == 0 {
            text.push('\n');
        }
        text.push_str(SENTENCE);
        line += 1;
    }
    text.truncate(bytes);
    text
}

fn binary(bytes: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; bytes];
    Rng::new(7).fill(&mut buffer);
    buffer
}

fn csharp(bytes: usize) -> String {
    const SNIPPET: &str = "public void UpsertEntries(IReadOnlyList<FsEntryRow> rows, long crawlGen)\n{\n    if (rows.Count == 0) return; // nothing to write\n    using var conn = _db.Open();\n    using var tx = conn.BeginTransaction();\n    foreach (var row in rows)\n    {\n        var key = row.PathKey ?? throw new InvalidOperationException(\"no key\");\n        cmd.Parameters[\"@size\"].Value = row.SizeBytes * 2 + 0x1F;\n        if (row.Hidden && !row.IsDirectory) count++;\n        text = $\"{row.Name}: {row.ModifiedUtc:O}\";\n    }\n    tx.Commit();\n}\n\n";
    let mut text = String::with_capacity(bytes + SNIPPET.len());
    while text.len() < bytes {
        text.push_str(SNIPPET);
    }
    text.truncate(bytes);
    text
}

fn json(bytes: usize) -> String {
    let mut text = String::from("[\n");
    let mut i = 0;
    while text.len() < bytes {
        text.push_str(&format!(
            "  {{\"id\": {i}, \"name\": \"item-{}\", \"size\": 12345.678, \"hidden\": false, \"tags\": [\"a\", \"b\", null], \"nested\": {{\"ok\": true}}}},\n",
            i + 1
        ));
        i += 1;
    }
    text.push_str("]\n");
    text.truncate(bytes);
    text
}

fn preview(suite: &mut Suite) {
    for kind in ["utf8", "utf16", "latin1", "binary"] {
        let params = [("Kind", kind)];
        suite.add(
            bench_id("textdecode", "decode", &params),
            &params,
            0.0,
            move || {
                let text = prose(MEGABYTE);
                let bytes: Vec<u8> = match kind {
                    "utf8" => text.into_bytes(),
                    "utf16" => {
                        let mut v = vec![0xFF, 0xFE];
                        v.extend(
                            text[..MEGABYTE / 2]
                                .encode_utf16()
                                .flat_map(u16::to_le_bytes),
                        );
                        v
                    }
                    "latin1" => text
                        .replace("the", "th\u{e9}")
                        .chars()
                        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
                        .collect(),
                    _ => binary(MEGABYTE),
                };
                let options = TextReadOptions::default();
                Ok(Op::new(move || {
                    black_box(TextPreviewReader::decode(&bytes, false, &options));
                }))
            },
        );
    }

    suite.add(bench_id("preview", "readtext1mb", &[]), &[], 0.0, || {
        let bytes = prose(MEGABYTE).into_bytes();
        let options = TextReadOptions::default();
        Ok(Op::new(move || {
            let len = bytes.len() as i64;
            black_box(TextPreviewReader::read(
                &mut Cursor::new(&bytes),
                len,
                &options,
            ));
        }))
    });
    suite.add(
        bench_id("preview", "readhex5000rows", &[]),
        &[],
        0.0,
        || {
            const ROWS: usize = 5_000;
            let bytes = binary(ROWS * 16);
            Ok(Op::new(move || {
                let len = bytes.len() as i64;
                black_box(HexPreviewReader::read(
                    &mut Cursor::new(&bytes),
                    len,
                    ROWS,
                    0,
                ));
            }))
        },
    );

    for (name, language) in [
        ("CSharp", SyntaxLanguage::CSharp),
        ("Json", SyntaxLanguage::Json),
    ] {
        let params = [("Language", name)];
        suite.add(
            bench_id("tokenize", "tokenize1mb", &params),
            &params,
            0.0,
            move || {
                let text = if language == SyntaxLanguage::Json {
                    json(MEGABYTE)
                } else {
                    csharp(MEGABYTE)
                };
                Ok(Op::new(move || {
                    black_box(SyntaxTokenizer::tokenize(&text, language));
                }))
            },
        );
    }
}

// ---- planners over a fake disk ----

fn planners(suite: &mut Suite) {
    const SOURCE: &str = r"Q:\Bench\Src";
    const DESTINATION: &str = r"Q:\Bench\Dst";

    // 600 files and 400 folders, 20 of them inside another selected folder: the containment check
    // the planner makes for every pair.
    suite.add(bench_id("transfer", "plan1000", &[]), &[], 0.0, || {
        let disk = FakeDisk::new();
        disk.add_directory(SOURCE).add_directory(DESTINATION);
        let mut sources = Vec::new();
        for i in 0..600 {
            let path = format!(r"{SOURCE}\file{i:03}.txt");
            disk.add_file(&path, 4096);
            sources.push(path);
        }
        for i in 0..400 {
            let path = format!(r"{SOURCE}\dir{i:03}");
            disk.add_directory(&path);
            sources.push(path);
        }
        for i in 0..20 {
            let path = format!(r"{SOURCE}\dir005\inner{i:02}");
            disk.add_directory(&path);
            sources.push(path);
        }
        let planner = TransferPlanner::new(Box::new(disk));
        Ok(Op::new(move || {
            black_box(planner.plan(&sources, DESTINATION, TransferVerb::Move));
        }))
    });

    // The merge: the same folder on both sides, 4,000 files in 40 subfolders, every one a clash.
    suite.add(
        bench_id("transfer", "expandmerge4000", &[]),
        &[],
        0.0,
        || {
            let disk = FakeDisk::new();
            disk.add_directory(SOURCE).add_directory(DESTINATION);
            for side in [SOURCE, DESTINATION] {
                disk.add_directory(&format!(r"{side}\Big"));
                for d in 0..40 {
                    disk.add_directory(&format!(r"{side}\Big\sub{d:02}"));
                    for f in 0..100 {
                        disk.add_file(&format!(r"{side}\Big\sub{d:02}\item{f:03}.dat"), 2048 + d);
                    }
                }
            }
            let planner = TransferPlanner::new(Box::new(disk.clone()));
            let expander = TransferMergeExpander::new(Box::new(disk));
            let plan = planner.plan(&[format!(r"{SOURCE}\Big")], DESTINATION, TransferVerb::Copy);
            let cancel = CancelToken::new();
            Ok(Op::new(move || {
                black_box(expander.expand(plan.clone(), &cancel).is_ok());
            }))
        },
    );

    suite.add(bench_id("rename", "planliteral1000", &[]), &[], 0.0, || {
        rename_op(true)
    });
    suite.add(
        bench_id("rename", "plannumbered1000", &[]),
        &[],
        0.0,
        || rename_op(false),
    );

    for (mode, name) in [
        (DeleteMode::Recycle, "Recycle"),
        (DeleteMode::Permanent, "Permanent"),
    ] {
        let params = [("Mode", name)];
        suite.add(
            bench_id("delete", "plan1000", &params),
            &params,
            0.0,
            move || {
                let disk = FakeDisk::new();
                disk.add_directory(SOURCE);
                let mut sources = Vec::new();
                for i in 0..900 {
                    let path = format!(r"{SOURCE}\f{i:04}.txt");
                    disk.add_file(&path, 1024);
                    sources.push(DeleteSource::new(&path, false));
                }
                for i in 0..50 {
                    let path = format!(r"{SOURCE}\d{i:02}");
                    disk.add_directory(&path);
                    sources.push(DeleteSource::new(&path, true));
                    let nested = format!(r"{path}\inner");
                    disk.add_directory(&nested);
                    sources.push(DeleteSource::new(&nested, true));
                }
                Ok(Op::new(move || {
                    let planner = DeletePlanner::new(&disk).recycle_probe(&disk);
                    black_box(planner.plan(&sources, mode));
                }))
            },
        );
    }
}

fn rename_op(literal: bool) -> Result<Op, String> {
    let disk = FakeDisk::new();
    disk.add_directory(r"Q:\Bench\Src");
    let sources: Vec<RenameSource> = (0..1000)
        .map(|i| {
            let path = format!(r"Q:\Bench\Src\f{i:04}.txt");
            disk.add_file(&path, 1024);
            RenameSource::file(&path)
        })
        .collect();
    let rule = RenameRule::new("{base}-{n:0000}{ext}");
    Ok(Op::new(move || {
        let planner = RenamePlanner::new(&disk);
        if literal {
            black_box(planner.plan_simple(&sources, "renamed.txt"));
        } else {
            black_box(planner.plan(&sources, &rule));
        }
    }))
}

// ---- GIF compositing ----

fn gif(suite: &mut Suite) {
    const WIDTH: i32 = 320;
    const HEIGHT: i32 = 240;

    fn opaque(width: i32, height: i32, rng: &mut Rng, transparent_every: usize) -> Vec<u8> {
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        rng.fill(&mut pixels);
        for (i, px) in pixels.chunks_mut(4).enumerate() {
            px[3] = if transparent_every > 0 && i % transparent_every == 0 {
                0
            } else {
                255
            };
        }
        pixels
    }

    fn frames() -> Vec<GifSubFrame> {
        let mut rng = Rng::new(11);
        let mut frames = vec![GifSubFrame {
            left: 0,
            top: 0,
            width: WIDTH,
            height: HEIGHT,
            pixels: opaque(WIDTH, HEIGHT, &mut rng, 0),
            disposal: GifDisposal::Keep,
        }];
        for i in 1..60usize {
            let w = rng.range(16, 120) as i32;
            let h = rng.range(16, 90) as i32;
            let disposal = match 1 + i % 3 {
                1 => GifDisposal::Keep,
                2 => GifDisposal::Clear,
                _ => GifDisposal::Restore,
            };
            let left = rng.range(0, i64::from(WIDTH - w)) as i32;
            let top = rng.range(0, i64::from(HEIGHT - h)) as i32;
            frames.push(GifSubFrame {
                left,
                top,
                width: w,
                height: h,
                pixels: opaque(w, h, &mut rng, 5),
                disposal,
            });
        }
        frames
    }

    suite.add(bench_id("gif", "next60frames", &[]), &[], 0.0, || {
        let frames = frames();
        Ok(Op::new(move || {
            let mut compositor = GifFrameCompositor::new(WIDTH as u32, HEIGHT as u32);
            let mut bytes = 0;
            for frame in &frames {
                bytes += compositor.next(frame).len();
            }
            black_box(bytes);
        }))
    });
    suite.add(bench_id("gif", "nextshared60frames", &[]), &[], 0.0, || {
        let frames = frames();
        Ok(Op::new(move || {
            let mut compositor = GifFrameCompositor::new(WIDTH as u32, HEIGHT as u32);
            let mut bytes = 0;
            for frame in &frames {
                bytes += compositor.next_shared(frame).len();
            }
            black_box(bytes);
        }))
    });
}
