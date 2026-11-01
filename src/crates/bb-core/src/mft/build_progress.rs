//! One percentage across the phases of a volume's initial build, so the status line can say how far
//! along it is. A build is three long loops one after another, each with an exact count of its own
//! and none with a count of the whole, so each phase gets a span of the bar. The spans are one real
//! system drive's shares of its time, fresh and over an existing index alike (100 s: reading 17%,
//! writing 79%, sizes 3%, finishing 1%). A small drive spends more of its time reading, and climbs
//! unevenly for the few seconds it takes. The count is of `$MFT` records, not of bytes or folders.

/// A long stretch of `build_from_raw_mft`, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildPhase {
    /// Reading the `$MFT`; counted in bytes of it.
    Reading,
    /// Writing `fs_entry`; counted in records.
    Writing,
    /// Writing `dir_size_cache`; counted in directories.
    Sizes,
}

impl BuildPhase {
    /// Where `Reading` ends on the bar.
    const READ_END: u64 = 17;
    /// Where `Writing` ends on the bar.
    const WRITE_END: u64 = 96;
    /// Where `Sizes` ends. Never 100: sweeping what vanished and registering the root still follow,
    /// and the number's last job is to disappear when they are done.
    const SIZES_END: u64 = 99;

    fn span(self) -> (u64, u64) {
        match self {
            BuildPhase::Reading => (0, Self::READ_END),
            BuildPhase::Writing => (Self::READ_END, Self::WRITE_END),
            BuildPhase::Sizes => (Self::WRITE_END, Self::SIZES_END),
        }
    }

    /// The whole percentage `done` of `total` of this phase comes to. A phase with nothing to do
    /// is already over.
    pub fn percent(self, done: u64, total: u64) -> u8 {
        let (from, to) = self.span();
        if total == 0 {
            return to as u8;
        }
        (from + (to - from) * done.min(total) / total) as u8
    }
}

/// Hands percentages to a callback, once per change. A message crosses the pipe and repaints the
/// window for every one of them, so a whole percent is as fine as this gets however often it is asked.
pub struct BuildProgress<'a> {
    report: &'a dyn Fn(Option<u8>),
    last: Option<u8>,
}

impl<'a> BuildProgress<'a> {
    pub fn new(report: &'a dyn Fn(Option<u8>)) -> BuildProgress<'a> {
        BuildProgress { report, last: None }
    }

    pub fn at(&mut self, phase: BuildPhase, done: u64, total: u64) {
        self.publish(Some(phase.percent(done, total)));
    }

    /// Takes the number back: the build fell back to the USN enumeration, which has no total, and a
    /// percentage left over from the read it abandoned would sit there being wrong until it ended.
    pub fn unknown(&mut self) {
        self.publish(None);
    }

    fn publish(&mut self, percent: Option<u8>) {
        if percent != self.last {
            self.last = percent;
            (self.report)(percent);
        }
    }
}
