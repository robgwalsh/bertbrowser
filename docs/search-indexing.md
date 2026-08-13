# Search indexing

How BertBrowser answers "find every file on this PC whose name contains *foo*" while you are still
typing. The short version: **it never searches the filesystem.** It searches a SQLite table that was
built by reading NTFS's own master index in one sequential pass, and that is patched in place by the
volume's change journal from then on.

Everything below is the detail behind that.

## The three layers

| Layer | What it does | Code |
|---|---|---|
| **Build** | One raw read of each NTFS volume's `$MFT` → every name, parent, size and timestamp on the disk | `Services/Mft/MftReader`, `MftVolumeIndexer` |
| **Maintain** | Tail the USN change journal and patch the index as files are created, renamed and deleted | `MftVolumeIndexer.Tail` |
| **Query** | An indexed range scan over one SQLite table | `Data/FsIndexRepository`, `Services/SearchService` |

Each layer is fast for a different reason, and all three have to hold for the search box to feel
instant. A fallback path (`IndexCrawler` + `FileSystemWatcher`) covers roots no NTFS volume
provides — network shares, exFAT sticks — and is described at the end.

## What the index is

One table, created in `Data/Migrations/002_search_index.sql`:

```sql
CREATE TABLE fs_entry (
    path_key     TEXT PRIMARY KEY,   -- "C:\USERS\ROB\NOTES.TXT" — canonical, uppercased
    name         TEXT NOT NULL,      -- "Notes.txt" — display casing
    name_key     TEXT NOT NULL,      -- "NOTES.TXT" — what queries match against
    is_dir       INTEGER NOT NULL,
    size_bytes   INTEGER NOT NULL DEFAULT 0,
    modified_utc TEXT NOT NULL,
    hidden       INTEGER NOT NULL DEFAULT 0,
    crawl_gen    INTEGER NOT NULL
) WITHOUT ROWID;
```

Four decisions in that schema do most of the work:

- **`WITHOUT ROWID` with `path_key` as the primary key** makes the table *itself* the clustered
  B-tree, ordered by path. There is no separate index to hop through and no rowid indirection: a
  subtree search is a contiguous walk of leaf pages.
- **There is deliberately no index on `name_key`.** A B-tree can accelerate a prefix match; it can
  do nothing for a *substring* match, which is what users actually type. An extra index would double
  the write cost of every build for zero query benefit.
- **`name_key` is uppercased in C#, not by SQLite.** SQLite's `upper()` and `NOCASE` fold ASCII
  only, so all case folding happens in `PathKey`/`SearchQuery` with `ToUpperInvariant`, and the
  columns compare with plain BINARY collation. This is the same invariant the rest of the database
  follows (see the path-key section of `CLAUDE.md`).
- **`hidden` is the *effective* flag** — the entry's own Hidden attribute OR'd down from every
  ancestor. Filtering hidden results is then `AND hidden = 0`, with no ancestor lookups at query
  time.

Note what is *not* stored: the display-cased full path. Only the uppercased key and the entry's own
display name are kept; a hit's real path is reassembled at query time from its ancestors' `name`
rows. Storing both casings of every path would roughly double the index.

## Build: reading the MFT directly

Every file on an NTFS volume already has an entry in a single system file, `$MFT`. Walking the
filesystem to find those names means millions of directory-open/enumerate syscalls, each with a
security check. Reading `$MFT` means opening `\\.\C:` and streaming one file.

`MftReader.TryReadAll` does exactly that:

1. **Read the boot sector** (offset 0) for the volume geometry: bytes per sector, bytes per cluster,
   the `$MFT`'s starting cluster, and bytes per FILE record (typically 1 KB). A missing `NTFS`
   signature or nonsense geometry returns `false` rather than throwing — the caller falls back.
2. **Read record 0**, which is `$MFT`'s own record, and decode its unnamed `$DATA` runlist. `$MFT`
   is itself fragmented, so this runlist is the extent map telling us where the rest of it lives.
3. **Stream those extents in 4 MB chunks**, slicing whole FILE records out of each chunk and
   carrying a partial record across the boundary when a record straddles clusters. Volume reads are
   issued cluster-aligned through `RandomAccess` — a hard requirement for raw `\\.\C:` access.
4. **Apply the update-sequence fixup** to each record. NTFS stamps a signature word into the last
   two bytes of every sector to detect torn writes; without undoing that, any field straddling a
   sector boundary is garbage.
5. **Parse the attributes** we care about: `$STANDARD_INFORMATION` for the modified timestamp and
   the Hidden bit, `$FILE_NAME` for the name and parent reference, `$DATA` for the real size.

Records that are skipped: numbers below 16 (the reserved metafiles — `$MFT`, `$LogFile`, the root's
`.`), records without the in-use flag (deleted), and extension records, whose attributes belong to a
base record parsed separately. Where a file has both a Win32 and an 8.3 DOS name, the non-DOS one
wins.

One case needs disk after all: a heavily fragmented file can have its unnamed `$DATA` attribute
pushed out into extension records, so the base record cannot report a size. The parser signals that
with `Size = -1` and `MftVolumeIndexer.StatFileSize` stats those few files individually.

The parsers — boot sector, fixup, FILE record — are pure static methods precisely so they can be
unit-tested against canned bytes without a volume (`NtfsParsingTests`).

### Rebuilding paths

An MFT record knows its own name and its parent's record number, and nothing else. There is no path
in the table. So after the read, every entry's path has to be reconstructed by walking parent links
up to the root (record 5).

Done naively that is O(entries × depth). `MftPathBuilder.TryResolve` memoizes every directory it
resolves into a shared cache, so the walk up stops at the first ancestor already known — resolving
one deep entry caches every directory along the way, and the next sibling short-circuits
immediately. In practice the whole volume resolves in roughly O(entries). A `MaxDepth` of 512 guards
against a cycle from a corrupt or racing table, and a broken parent chain (an ancestor deleted
mid-read) just skips that entry rather than failing the build.

That same directory map — record number → (path, effective-hidden) — is kept resident afterwards. It
is what lets the journal tail resolve a change record's path without touching disk.

### Directory sizes come free

The same record stream feeds `MftDirectorySizeBuilder`, which buckets each file's bytes onto its
direct parent and then does one iterative post-order pass to roll subtree totals up the tree. That
produces a `dir_size_cache` row for **every directory on the volume** as a side effect of building
the search index — which is why folder sizes appear instantly in the tree instead of being scanned
for. Because the raw read sees every record regardless of ACLs, those results are never marked
`incomplete`.

Junctions and other reparse points have no MFT children under their own record, so they contribute
nothing, which is what stops a junction being counted twice.

This is the *only* source of directory sizes — there is no on-demand scanner to fall back on, so a
directory with no row here reads as unknown rather than zero.

### Writing it down

Rows are upserted in chunks of 20,000, one transaction each, so a multi-million-entry volume never
buffers the whole tree in memory and never blocks other writers behind one giant transaction.

Every write is stamped with `crawl_gen`, a unix-millisecond value fixed at the start of the build.
When the build completes, `SweepVanished` deletes rows in the volume's range whose stamp is *older*
than the build — those are entries that no longer exist. Live journal writes stamp the current time,
so they always survive a concurrent sweep. Only then is the root registered `complete`.

A cancelled build sweeps nothing and marks nothing complete; the rows it already wrote are harmless
real data.

### Orchestration

`MftIndexService.Start` enumerates fixed NTFS volumes (`GetLogicalDrives` → `GetDriveTypeW` →
`GetVolumeInformationW`) and gives each one its own background thread: build, then tail, for the
life of the app. A volume that fails to open, has no usable journal, or throws mid-build is
non-fatal — searches on it fall back to the crawler. `StatusText` drives the "Indexing C:…" note in
the status bar, and `IndexRefreshed` lets an already-open search re-query once a volume lands.

**This is why the app requests `requireAdministrator`.** Opening `\\.\C:` for raw reads needs it.
Nothing else in BertBrowser does — including the programs it launches, which are started by the
desktop shell at your own integrity level rather than inheriting this one. See
[SECURITY.md](../SECURITY.md).

## Maintain: the USN journal

A build is a snapshot. Keeping it true without ever rescanning is the job of the NTFS change
journal, which records every filesystem mutation on the volume with a monotonically increasing USN.

`MftVolumeIndexer.Open` queries the journal, creating one (32 MB, 4 MB allocation delta) if the
volume has none. `Tail` then loops on `FSCTL_READ_USN_JOURNAL` from the last USN it consumed,
sleeping 1 s when there is nothing new.

Applying a batch (`Apply`) is where the subtlety is:

- **Only `CLOSE` records are acted on.** A single file write produces a flurry of reason bits; the
  coalescing close record is the one worth reacting to, so everything else is dropped.
- **`RENAME_OLD_NAME` is the exception** — it carries the pre-rename path, is captured into
  `_pendingRenames`, and is paired with the matching new-name record that follows.
- **Paths are resolved from the resident directory map**, not from disk. Records arrive in USN
  order, so a directory's creation always precedes its children's.
- **A directory rename is a prefix rewrite, not a re-crawl.** `FsIndexRepository.Rename` updates the
  moved row and rewrites every descendant key with one `UPDATE … SET path_key = @new || substr(...)`
  over the old subtree's range. The in-memory directory map is rewritten to match.
- **A delete removes the entry and its whole subtree** via the same range bounds.
- If reading the journal fails at all — overflow, journal deleted, ID changed — the root is marked
  **stale** and the tail exits, so the next search serves cached results instantly while a rebuild
  runs.

## Query: what happens when you type

`SearchQuery.Parse` splits the input on whitespace into terms that are ANDed, uppercases them, and
**returns null below two literal (non-wildcard) characters** — a one-character substring search over
a whole PC is not a useful query. `*` and `?` are passed through as wildcards.

A scoped search (`FsIndexRepository.Search`) is then one statement:

```sql
SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden
FROM fs_entry
WHERE path_key >= @lo AND path_key < @hi AND name_key GLOB @g0 [AND name_key GLOB @g1 …]
LIMIT @limit;
```

- **`@lo`/`@hi` come from `PathKey.PrefixBounds`**, a half-open range `[dir\, dir])` — `]` is the
  character immediately after `\` in ASCII, so "everything under this directory" is a pure index
  range scan on the clustered key rather than a `LIKE 'dir%'` full scan.
- **The GLOB patterns are `*TERM*`**, with `[` escaped as `[[]` since it opens a character class.
  This is a linear substring test, but only over rows already inside the range.
- **There is no `ORDER BY`.** That is deliberate: with `LIMIT`, SQLite can stop the moment it has
  enough rows, whereas any sort forces the whole matching set to be materialised first. The capped
  page (1,000 hits) is sorted in the caller, where it is cheap.
- **The limit is `cap + 1`.** If the extra row comes back, the result set was truncated; it is
  dropped and the status bar says "showing first 1,000".

A whole-PC search (`SearchGlobal`) is the identical query with the range bound removed, so it scans
every indexed volume.

### Rebuilding display paths

Since full display paths are not stored, each hit's path is reassembled: `LookupAncestorNames`
collects every distinct ancestor key implied by the result page and fetches their `name` values in
batches of 500 `IN (…)` lookups, then `BuildRelativeDir` joins them. Bounded by the page size, this
is a handful of point lookups regardless of how large the index is. A missing ancestor row falls
back to the uppercase key segment rather than losing the row.

### And in the UI

`DirectoryTabViewModel` debounces typing by 200 ms and cancels the in-flight query on every
keystroke, so a fast typist runs one search, not twelve. Search deliberately never surfaces hidden
entries regardless of the "Show hidden items" browse setting — `AppData` and system noise bury the
results a search is actually for. Global hits are hydrated with size/timestamp from disk after the
fact, since the fallback enumeration path can leave those blank.

`SearchService.Visible` also drops any hit sitting in a delete's holding folder. Those files are
still on disk — that is the whole point, so Ctrl+Z can restore them — but they have been deleted as
far as the user is concerned, and search saying otherwise reads as a delete that silently failed.

## The fallback path

Not everything is a fixed NTFS volume. For roots the MFT indexer does not cover, `SearchService`
runs stale-while-revalidate:

| Index state | What the user gets |
|---|---|
| Fresh (complete, not stale, and something is patching it live) | Straight DB query |
| Stale or unwatched | Straight DB query **plus** a background re-crawl |
| Not indexed at all | Live filesystem walk streaming hits in batches of 50 (or every 250 ms) while a single-flight background crawl indexes the subtree for next time |

`IndexCrawler` writes the same `fs_entry` rows with the same chunking and `crawl_gen` contract, and
`IndexWatcherService` keeps up to 8 roots patched with `FileSystemWatcher` (events queued and drained
every 250 ms; a buffer overflow marks the root stale and drops the watcher). `FileSystemWatcher`
state is in-memory only, so the *first* crawl-backed search each session intentionally takes the
stale path: instant cached results, plus one background re-crawl that re-attaches the watcher.
MFT-covered roots never fall to the crawler, since the USN tail keeps them live.

Typing more characters never restarts a crawl — `EnsureIndexed` is single-flight per root, keyed on
the canonical path.

## Costs and limits

- **Admin rights**, for raw volume access. Non-negotiable for the MFT path.
- **Fixed NTFS volumes only.** Network shares, exFAT/FAT32 removable media and MTP devices go
  through the crawler (or, for MTP, are not searchable at all).
- **Index size** is one row per file and directory on every fixed volume, in
  `%USERPROFILE%\.bertbrowser\bertbrowser.db`. Deleting that folder resets it; the next launch
  rebuilds.
- **Results are capped at 1,000** per query.
- **Names only.** There is no content indexing, and none is planned.
- **A full rebuild runs at every launch.** It is a sequential read, it is off-thread, and searches
  work against the previous contents while it runs — but it is not free on a very large disk.

## Where the tests are

| Test | Covers |
|---|---|
| `NtfsParsingTests` | Boot-sector geometry, runlist decoding, the sector fixup, FILE-record parsing |
| `UsnRecordParserTests` | Journal record parsing |
| `MftPathBuilderTests` | Parent-chain resolution, memoization, broken chains, hidden inheritance |
| `MftDirectorySizeBuilderTests` | Post-order rollup totals |
| `SearchQueryTests` | Term parsing, the two-literal-character floor, wildcard matching, GLOB escaping |
| `FsIndexRepositoryTests` | Range scans, truncation, ancestor path reconstruction, rename/delete subtree rewrites, the vanish sweep |
| `SearchServiceTests` | Fresh / stale / unindexed routing and live-scan streaming |
| `IndexCrawlerTests`, `IndexWatcherApplyTests` | The fallback crawler and watcher apply path |
