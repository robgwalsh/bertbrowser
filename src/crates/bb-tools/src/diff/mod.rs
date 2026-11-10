//! Comparing two texts line by line. Spec: Core/Services/Diff/DiffModels.cs, TextDiffer.cs.
//!
//! Myers' O(ND) algorithm, with the two standard reductions: the common prefix and suffix are
//! trimmed before it starts, and lines are reduced to integer keys so the inner loop never
//! touches a string. Not patience diff: its advantage is readability on moved code, but it needs
//! unique-line anchoring *plus* a Myers fallback anyway, which is twice the code for a better
//! answer on a minority of cases.
//!
//! **D is bounded, and the fallback is honest.** Myers costs O(ND), and D is the size of the edit
//! script: small for two versions of the same file, which is the case it is for, and as large as
//! both files put together for two files that share nothing. Past the budget it stops and returns
//! a plain positional pairing with `budget_exceeded` set, so the window can say "too different to
//! align" rather than hanging.

use std::collections::HashMap;

/// What one displayed row of a side-by-side diff is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffOp {
    /// The same line on both sides.
    Equal,
    /// On the left only; the right cell is blank.
    Delete,
    /// On the right only; the left cell is blank.
    Insert,
    /// A line on each side, paired: this line became that one.
    Replace,
}

/// One row of the side-by-side view. Line numbers are 1-based, 0 meaning "nothing on this side".
/// Numbers rather than text, so the rows for a large file cost a few bytes each and the view
/// reads the strings it already has. The pairing has already happened by the time these exist: a
/// run of deletions beside a run of insertions has been folded into `Replace` rows, so changed
/// lines sit opposite each other instead of stacking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub op: DiffOp,
    pub left_line: usize,
    pub right_line: usize,
}

/// A run of changed rows with the equal rows either side of it left out. `first_row` is an index
/// into [`TextDiff::rows`], so prev/next-difference is a scroll to a row the view already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub first_row: usize,
    pub row_count: usize,
    pub left_line: usize,
    pub right_line: usize,
}

/// What comparing two texts found. `budget_exceeded` says the two texts were too far apart to
/// align within the work budget, so `rows` is a plain positional pairing rather than an edit
/// script; said out loud because it changes what the rows mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextDiff {
    pub rows: Vec<DiffRow>,
    pub hunks: Vec<DiffHunk>,
    pub left_line_count: usize,
    pub right_line_count: usize,
    /// Rows that are not `Equal`.
    pub changed_line_count: usize,
    pub budget_exceeded: bool,
}

impl TextDiff {
    pub fn identical(&self) -> bool {
        self.changed_line_count == 0
    }
}

/// What counts as the same line. `ignore_trailing_whitespace` is on by default: line endings are
/// already normalised by the reader, and a difference of trailing spaces is the one difference a
/// person cannot see and will not believe. Deliberately not offered: ignoring blank lines, which
/// desynchronises the gutter numbers people are reading off, and anything language-aware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffOptions {
    pub ignore_trailing_whitespace: bool,
    pub ignore_all_whitespace: bool,
    pub ignore_case: bool,
}

impl Default for DiffOptions {
    fn default() -> DiffOptions {
        DiffOptions {
            ignore_trailing_whitespace: true,
            ignore_all_whitespace: false,
            ignore_case: false,
        }
    }
}

/// One step of the edit script, in line indices of the trimmed middle.
#[derive(Clone, Copy)]
enum Step {
    Equal(usize, usize),
    Delete(usize),
    Insert(usize),
}

pub struct TextDiffer;

impl TextDiffer {
    /// The furthest apart two texts may be before alignment is abandoned. The backtrack needs a
    /// snapshot of the frontier per step, which costs about (D+1)² integers, roughly 16 MB here.
    /// Two versions of one file are a handful of edits apart and never come near it; two
    /// unrelated files exceed it immediately, and for those a line diff was never going to say
    /// anything anyway.
    pub const MAX_EDIT_DISTANCE: usize = 2048;

    /// Lines beyond this on either side are not aligned. A file this long is a log.
    pub const MAX_LINES: usize = 200_000;

    /// How many unchanged lines are kept either side of a change when grouping hunks.
    pub const CONTEXT_LINES: usize = 3;

    pub fn compare(left: &[&str], right: &[&str], options: &DiffOptions) -> TextDiff {
        if left.len() > Self::MAX_LINES || right.len() > Self::MAX_LINES {
            return Self::positional(left.len(), right.len(), true);
        }

        // One pool across both sides, or the ids mean nothing: two separate pools would number
        // each side's first distinct line 0, and every line would "match" the line opposite it.
        let mut pool: HashMap<String, usize> = HashMap::new();
        let a = Self::keys(left, options, &mut pool);
        let b = Self::keys(right, options, &mut pool);

        // Trim what matches at both ends. This is what makes a one-line change in a
        // ten-thousand-line file cost almost nothing: everything but the changed region
        // disappears before Myers starts.
        let mut prefix = 0;
        while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
            prefix += 1;
        }

        let mut suffix = 0;
        while suffix < a.len() - prefix
            && suffix < b.len() - prefix
            && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
        {
            suffix += 1;
        }

        let middle_a = &a[prefix..a.len() - suffix];
        let middle_b = &b[prefix..b.len() - suffix];

        let Some(script) = Self::myers(middle_a, middle_b) else {
            return Self::positional(left.len(), right.len(), true);
        };

        let mut rows: Vec<DiffRow> = Vec::with_capacity(left.len() + right.len());

        for i in 0..prefix {
            rows.push(DiffRow {
                op: DiffOp::Equal,
                left_line: i + 1,
                right_line: i + 1,
            });
        }

        Self::pair(&script, prefix, &mut rows);

        for i in 0..suffix {
            let l = a.len() - suffix + i;
            let r = b.len() - suffix + i;
            rows.push(DiffRow {
                op: DiffOp::Equal,
                left_line: l + 1,
                right_line: r + 1,
            });
        }

        Self::finish(rows, left.len(), right.len(), false)
    }

    /// One line reduced to what "the same line" means under these options. Interned to an
    /// integer so Myers compares integers: two lines that normalise the same get the same key,
    /// which is how the options cost nothing per comparison.
    fn keys(
        lines: &[&str],
        options: &DiffOptions,
        pool: &mut HashMap<String, usize>,
    ) -> Vec<usize> {
        lines
            .iter()
            .map(|line| {
                let key = Self::normalise(line, options);
                let next = pool.len();
                *pool.entry(key).or_insert(next)
            })
            .collect()
    }

    /// Applied to the *key* only. The text a view shows is always the original: a normalisation
    /// that changed what was displayed would be editing the file on screen.
    fn normalise(line: &str, options: &DiffOptions) -> String {
        let text: String = if options.ignore_all_whitespace {
            line.chars().filter(|c| !c.is_whitespace()).collect()
        } else if options.ignore_trailing_whitespace {
            line.trim_end().to_string()
        } else {
            line.to_string()
        };

        if options.ignore_case {
            text.chars().flat_map(char::to_lowercase).collect()
        } else {
            text
        }
    }

    /// The edit script, or None when the two are further apart than the budget allows.
    fn myers(a: &[usize], b: &[usize]) -> Option<Vec<Step>> {
        let (n, m) = (a.len() as isize, b.len() as isize);
        let max_d = Self::MAX_EDIT_DISTANCE.min((n + m) as usize) as isize;

        let offset = max_d + 1;
        let mut v = vec![0isize; (2 * max_d + 3) as usize];
        let mut trace: Vec<Vec<isize>> = Vec::with_capacity((max_d as usize + 1).min(64));

        for d in 0..=max_d {
            // Only the diagonals this step can read, -d-1 to d+1: the rest of v is still zero,
            // and cloning all of it every step cost a full-width array per unit of edit distance.
            let lo = (offset - d - 1) as usize;
            trace.push(v[lo..lo + (2 * d + 3) as usize].to_vec());

            let mut k = -d;
            while k <= d {
                let mut x = if k == -d
                    || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
                {
                    v[(k + 1 + offset) as usize]
                } else {
                    v[(k - 1 + offset) as usize] + 1
                };

                let mut y = x - k;
                while x < n && y < m && a[x as usize] == b[y as usize] {
                    x += 1;
                    y += 1;
                }

                v[(k + offset) as usize] = x;

                if x >= n && y >= m {
                    return Some(Self::backtrack(&trace, a, b));
                }
                k += 2;
            }
        }

        None
    }

    fn backtrack(trace: &[Vec<isize>], a: &[usize], b: &[usize]) -> Vec<Step> {
        let mut script: Vec<Step> = Vec::new();
        let (mut x, mut y) = (a.len() as isize, b.len() as isize);

        for d in (0..trace.len()).rev() {
            // Step d's slice starts at diagonal -d-1, so diagonal k sits at k + d + 1.
            let v = &trace[d];
            let d = d as isize;
            let offset = d + 1;
            let k = x - y;

            let previous_k = if k == -d
                || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
            {
                k + 1
            } else {
                k - 1
            };

            let previous_x = v[(previous_k + offset) as usize];
            let previous_y = previous_x - previous_k;

            while x > previous_x && y > previous_y {
                script.push(Step::Equal((x - 1) as usize, (y - 1) as usize));
                x -= 1;
                y -= 1;
            }

            if d == 0 {
                break;
            }

            if x == previous_x {
                y -= 1;
                script.push(Step::Insert(y as usize));
            } else {
                x -= 1;
                script.push(Step::Delete(x as usize));
            }
        }

        script.reverse();
        script
    }

    /// Folds a run of deletions beside a run of insertions into paired `Replace` rows. Without
    /// this, three changed lines render as three blank-right rows followed by three blank-left
    /// rows, and the reader has to do the pairing in their head. The remainder of the longer run
    /// stays as plain deletions or insertions, which is honest: four lines becoming two is two
    /// replacements and two deletions, not four of anything.
    fn pair(script: &[Step], shift: usize, rows: &mut Vec<DiffRow>) {
        let mut deletes: Vec<usize> = Vec::new();
        let mut inserts: Vec<usize> = Vec::new();

        fn flush(
            deletes: &mut Vec<usize>,
            inserts: &mut Vec<usize>,
            shift: usize,
            rows: &mut Vec<DiffRow>,
        ) {
            let count = deletes.len().max(inserts.len());
            for i in 0..count {
                let left = deletes.get(i).map_or(0, |d| d + shift + 1);
                let right = inserts.get(i).map_or(0, |n| n + shift + 1);
                rows.push(DiffRow {
                    op: op_for(left, right),
                    left_line: left,
                    right_line: right,
                });
            }
            deletes.clear();
            inserts.clear();
        }

        for step in script {
            match *step {
                Step::Delete(a) => deletes.push(a),
                Step::Insert(b) => inserts.push(b),
                Step::Equal(a, b) => {
                    flush(&mut deletes, &mut inserts, shift, rows);
                    rows.push(DiffRow {
                        op: DiffOp::Equal,
                        left_line: a + shift + 1,
                        right_line: b + shift + 1,
                    });
                }
            }
        }

        flush(&mut deletes, &mut inserts, shift, rows);
    }

    /// Line *i* against line *i*, for when alignment was abandoned. Never an empty result and
    /// never a failure: showing the two side by side anyway is still more use than showing
    /// nothing, and the hunks it produces still let prev/next-difference work.
    fn positional(left_count: usize, right_count: usize, budget_exceeded: bool) -> TextDiff {
        let rows: Vec<DiffRow> = (0..left_count.max(right_count))
            .map(|i| {
                let left = if i < left_count { i + 1 } else { 0 };
                let right = if i < right_count { i + 1 } else { 0 };
                DiffRow {
                    op: op_for(left, right),
                    left_line: left,
                    right_line: right,
                }
            })
            .collect();

        Self::finish(rows, left_count, right_count, budget_exceeded)
    }

    fn finish(
        rows: Vec<DiffRow>,
        left_count: usize,
        right_count: usize,
        budget_exceeded: bool,
    ) -> TextDiff {
        let changed = rows.iter().filter(|r| r.op != DiffOp::Equal).count();
        let hunks = Self::hunks(&rows);

        TextDiff {
            rows,
            hunks,
            left_line_count: left_count,
            right_line_count: right_count,
            changed_line_count: changed,
            budget_exceeded,
        }
    }

    /// Groups changed rows into the blocks prev/next-difference steps through. A hunk opens at a
    /// change and closes once `CONTEXT_LINES` unchanged rows have gone by; two hunks nearer than
    /// twice that are merged, because stepping between two changes three lines apart is a jump
    /// that does not move the screen.
    fn hunks(rows: &[DiffRow]) -> Vec<DiffHunk> {
        let mut hunks: Vec<DiffHunk> = Vec::new();

        let mut start: Option<usize> = None;
        let mut last_change = 0;

        for (i, row) in rows.iter().enumerate() {
            if row.op == DiffOp::Equal {
                continue;
            }

            match start {
                None => start = Some(i),
                Some(from) if i - last_change > 2 * Self::CONTEXT_LINES => {
                    hunks.push(Self::make_hunk(rows, from, last_change));
                    start = Some(i);
                }
                Some(_) => {}
            }

            last_change = i;
        }

        if let Some(from) = start {
            hunks.push(Self::make_hunk(rows, from, last_change));
        }

        hunks
    }

    fn make_hunk(rows: &[DiffRow], from: usize, to: usize) -> DiffHunk {
        let row = rows[from];
        DiffHunk {
            first_row: from,
            row_count: to - from + 1,
            left_line: row.left_line,
            right_line: row.right_line,
        }
    }
}

fn op_for(left: usize, right: usize) -> DiffOp {
    if left > 0 && right > 0 {
        DiffOp::Replace
    } else if left > 0 {
        DiffOp::Delete
    } else {
        DiffOp::Insert
    }
}
