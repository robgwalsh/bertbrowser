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

**This is why there is a separate elevated process.** Opening `\\.\C:` for raw reads needs an
administrator token, and nothing else in BertBrowser does — so the app itself is `asInvoker` and
everything described above runs inside `BertBrowser.Indexer.exe`, which the app starts on demand and
talks to over a named pipe. The app holds an `MftIndexClient` that mirrors what the helper reports,
so `IsIndexed`, `AnyIndexed` and `StatusText` answer locally and none of the consumers above know
the difference. If the helper cannot run — a declined prompt, a standard-user account — nothing is
indexed and every search falls back to the crawler, which is the same path a non-NTFS volume takes.
See [SECURITY.md](../SECURITY.md) and the "elevated index helper" section of `CLAUDE.md`.

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

### The query language

A bare word means what it always did — a case-insensitive substring of the name, `*` and `?`
honoured, several of them ANDed — and on top of that `Core/Services/Search` adds filters:

| Form | Meaning |
|---|---|
| `report 2026` | both words, in any order |
| `"annual report"` | a literal phrase; wildcards lose their meaning inside quotes |
| `ext:jpg;png` | one of these extensions |
| `size:>100mb` | also `<`, `>=`, `<=`, `=`, `1mb..2mb`, and `empty` |
| `dm:today` | also `yesterday`, `thisweek`, `last7days`, `2026`, `2026-08`, `>2026-01-01`, `A..B` |
| `path:projects` | a substring of the whole path, not just the name |
| `is:dir` | also `is:file`, `is:hidden` |
| `re:^IMG_\d+` | a regular expression over the name |
| `!tmp`, `NOT tmp` | exclusion |
| `draft OR final` | alternation; adjacency is still AND, and AND binds tighter |
| `(a OR b) ext:txt` | brackets group |

Four rules keep this from breaking what people already type:

- **An unrecognised `key:` is ordinary text.** `C:\Users` pasted into the box still searches for
  that text. Only the keys in `SearchSyntax` mean anything, which is the same trade the advanced
  rename made by treating `{` as a token only in advanced mode.
- **`OR`/`NOT`/`AND` are operators only in uppercase**, so a file called `Report or Draft` stays
  findable and no existing query silently changes meaning.
- **A stray `!`, an unbalanced `)` and an unclosed `(` are all literal or forgiving** rather than
  errors. Quoting is the escape where a name really contains a bracket.
- **`dc:`/`da:`/`content:` are refused with a message.** They plainly mean something this index
  cannot answer, and degrading them to a substring search would return nothing while implying the
  disk holds no such files.

`SearchGrammar.Parse` **never throws** — a bad regular expression, an unreadable size, a half-typed
`size:>` all come back as text, because this runs on the UI thread on a keystroke. It returns three
outcomes, not two: a query, a *problem* (the view stays in search mode and shows the banner), or
nothing at all (the view goes back to the directory listing). The floor is unchanged — two literal
characters, **summed across the query** so `a b` still clears it — except that a filter specific
enough to stand alone (`ext:jpg`) now clears it with no text at all, while `is:dir` deliberately
does not.

### The two faces of a query, and why they cannot drift

`SearchNode` has **two** abstract members: `Matches(in SearchCandidate)` and `WriteSql`. A new
filter key cannot be added with only one wired up — it is a compile error, not a silent difference
between an indexed drive and a live scan.

Beyond that, **`Matches` is the definition and the SQL is only an optimisation**: the repository
re-applies `Matches` to every row it reads back. SQL that is too *wide* costs a longer scan and
nothing else; SQL that is too *narrow* drops rows and `SearchAgreementTests` — which runs ~40
queries through both paths over one corpus and compares — goes red. This is what lets `re:` compile
to `1`.

Two consequences fall out of that and are easy to undo by accident:

- **`LIMIT` is pushed down only when the predicate is exact.** An incomplete one returns a superset,
  so stopping at `cap + 1` rows would cap the wrong population — the rows the scan happened to
  reach, most of which the re-check discards. `ReadMatchingRows` counts rows that actually matched.
- **A superset cannot be negated.** `NOT (something wider than the truth)` is *narrower* than the
  truth and drops real matches, so `NotNode` emits `1` when its child's SQL is inexact. Without it
  `!re:foo` compiles to `NOT 1` and returns nothing.

### The SQL

A scoped search (`FsIndexRepository.Search`) is one statement:

```sql
SELECT path_key, name, is_dir, size_bytes, modified_utc, hidden, name_key
FROM fs_entry
WHERE path_key >= @lo AND path_key < @hi AND (<compiled predicate>)
LIMIT @limit;          -- only when the predicate is exact
```

- **`@lo`/`@hi` come from `PathKey.PrefixBounds`**, a half-open range `[dir\, dir])` — `]` is the
  character immediately after `\` in ASCII, so "everything under this directory" is a pure index
  range scan on the clustered key rather than a `LIKE 'dir%'` full scan.
- **A name term compiles to `name_key GLOB '*TERM*'`**, with `[` escaped as `[[]` since it opens a
  character class. `ext:` rides the same scan (`name_key GLOB '?*.JPG'`); `size:`, `dm:`, `is:` and
  `path:` are comparisons on columns already in the row.
- **There is no `ORDER BY`.** That is deliberate: with `LIMIT`, SQLite can stop the moment it has
  enough rows, whereas any sort forces the whole matching set to be materialised first. The capped
  page (1,000 hits) is sorted in the caller, where it is cheap.
- **The limit is `cap + 1`.** If the extra row comes back, the result set was truncated; it is
  dropped and the status bar says "showing first 1,000".
- **`modified_utc` is TEXT written with `"O"`** — fixed-width and zero-padded — so BINARY collation
  is already a correct chronological order and a date bound is a plain string comparison. Rows from
  the fallback build carry `0001-01-01…`, which sorts below the 1601 floor every date term applies,
  so an unmeasured row satisfies no date filter rather than matching every open-ended one.

A whole-PC search (`SearchGlobal`) is the identical query with the range bound removed, so it scans
every indexed volume.

### What that costs, measured

A filter is a predicate on a scan that was happening anyway, never a seek: **there is no index on
`size_bytes` or `modified_utc`, and adding one is refused** for the reason spelled out below for
duplicates. So a selective filter costs a *longer* scan before the 1,000-row cap fills, and one that
matches nothing costs a full scan.

Against a real 1,646 MB index — 1,912,992 rows, 1,601,024 of them with a length — read-only, warm:

| Query | Scoped | Global | Plan |
|---|---|---|---|
| `report` | 278 ms | 230 ms | range scan / `SCAN fs_entry` |
| `ext:txt` | 27 ms | 28 ms | " |
| `dm:today` | 81 ms | 25 ms | " |
| `size:>100mb` | 262 ms | 482 ms | " |
| `size:>100gb` (matches nothing — worst case) | 257 ms | 508 ms | " |
| `report ext:cs size:>1kb` | 295 ms | 614 ms | " |
| `re:^img_` (no `LIMIT` pushdown) | 1,216 ms | 2,460 ms | " |

`EXPLAIN QUERY PLAN` reports `SEARCH fs_entry USING PRIMARY KEY` scoped and `SCAN fs_entry` global
for every one of them, and **no temp B-tree anywhere**. A regular expression is the slow one and
visibly so: it cannot stop early, so it always reads the whole scope.

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

### The other reader: duplicates

The search box is not the only thing that reads these rows. The duplicate finder
(`Core/Services/Duplicates`) shortlists on `size_bytes` — two files of different lengths cannot be
duplicates — so the expensive half of finding them is already paid for by the pass above, and only
the files that collide are ever opened.

Two consequences worth knowing here rather than there. It is **two streaming scans with no
`GROUP BY`**, because there is no index on `size_bytes` and grouping would make SQLite materialise a
temp B-tree over most of every qualifying row; an index was refused for the same reason the one on
`name_key` was — `WITHOUT ROWID` means a secondary index carries the whole `path_key`, and the build
that would have to write it runs at every launch. And the **`FSCTL_ENUM_USN_DATA` fallback disables
the feature outright**: that path writes every row with `size_bytes = 0`, so every file would collide
with every other. The shortlist counts sized rows as it walks and reports `NoSizeData` rather than
reading a whole disk to discover nothing.

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

- **Admin rights**, for raw volume access. Non-negotiable for the MFT path — but confined to
  `BertBrowser.Indexer.exe`, so it costs one UAC prompt per session rather than an elevated browser.
  Declining is survivable: everything falls back to the crawler.
- **Fixed NTFS volumes only.** Network shares, exFAT/FAT32 removable media and MTP devices go
  through the crawler (or, for MTP, are not searchable at all).
- **Index size** is one row per file and directory on every fixed volume, in
  `%USERPROFILE%\.bertbrowser\bertbrowser.db`. Deleting that folder resets it; the next launch
  rebuilds.
- **Results are capped at 1,000** per query.
- **Names only.** There is no content indexing, and none is planned. `content:` says so rather than
  searching for the literal text.
- **`size:` and `dm:` need a fully indexed drive.** The `FSCTL_ENUM_USN_DATA` fallback records names
  only, so those filters can never match on a volume built that way. A filtered search that comes
  back empty asks `FsIndexRepository.HasSizeData` before reporting "no results", and says the drive
  is unmeasured instead — the same distinction `DiskUsageRules` draws, and for the same reason.
- **Only modified time is indexed**, not created or accessed; `dc:`/`da:` are refused with a message
  pointing at `dm:`.
- **A full rebuild runs at every launch.** It is a sequential read, it is off-thread, and searches
  work against the previous contents while it runs — but it is not free on a very large disk.

## Where the tests are

| Test | Covers |
|---|---|
| `NtfsParsingTests` | Boot-sector geometry, runlist decoding, the sector fixup, FILE-record parsing |
| `UsnRecordParserTests` | Journal record parsing |
| `MftPathBuilderTests` | Parent-chain resolution, memoization, broken chains, hidden inheritance |
| `MftDirectorySizeBuilderTests` | Post-order rollup totals |
| `SearchQueryTests` | What a bare query has always meant: the two-literal-character floor, wildcards, GLOB escaping — plus the compatibility cases the filter syntax could have broken silently (an unrecognised key, lowercase `or`, a trailing `!`) |
| `SearchGrammarTests` | The filter language: every key, the operators, the refusals, a real catastrophic backtrack |
| `SearchAgreementTests` | ~40 queries run through **both** SQL and the matcher over one corpus, asserted identical — the test that makes drift impossible — plus the cap and the size-data probe |
| `SizeTextTests`, `DateShorthandTests` | The literal parsers; the clock is injected, and the units are pinned to `ByteSizeFormatter`'s |
| `FsIndexRepositoryTests` | Range scans, truncation, ancestor path reconstruction, rename/delete subtree rewrites, the vanish sweep |
| `SearchServiceTests` | Fresh / stale / unindexed routing and live-scan streaming |
| `IndexCrawlerTests`, `IndexWatcherApplyTests` | The fallback crawler and watcher apply path |
