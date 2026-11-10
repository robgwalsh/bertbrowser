//! What the preview pane is asked to show, what the user asked over the top of it, and why it may
//! show nothing. Spec: Core/Services/Preview/PreviewKind.cs.

use super::SyntaxLanguage;

/// How a file should be shown in the preview pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PreviewKind {
    /// Nothing to show: the selection is empty, or it was refused.
    None,

    /// Decoded to a bitmap and shown, with the shell as a fallback decoder.
    Image,

    /// Read as text and shown with syntax colouring.
    Text,

    /// Audio or video: a poster frame, and a transport the user must press.
    Media,

    /// Listed entry by entry, without extracting anything.
    Archive,

    /// Rendered as a specimen using the face in the file itself.
    Font,

    /// Everything else, including PDF and Office: whatever preview the shell can produce. Also
    /// the fall-back for an unrecognised extension, which is why an unknown file gets a real
    /// attempt rather than an immediate refusal.
    Document,

    /// A hex dump: offset, bytes, ASCII. Never chosen by the extension table; it is only ever
    /// what `PreviewMode::Hex` asked for.
    Hex,
}

/// What the user asked the pane to show, over the top of what the file claims to be.
///
/// The override is applied *after* the classifier's refusals, never before: a folder and a cloud
/// placeholder are still refused, because hexing a placeholder would trigger exactly the
/// multi-gigabyte download the pane exists to avoid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PreviewMode {
    /// Let the extension decide, which is what the pane has always done.
    #[default]
    Auto,

    /// Decode the bytes as text whatever the file claims to be, and do not colour them. Total
    /// Commander's Lister calls this binary mode; it is how you read a PDF's header.
    Text,

    /// A hex dump.
    Hex,
}

/// Why there is no preview. `None` means there is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PreviewRefusal {
    None,

    /// Nothing is selected.
    NothingSelected,

    /// Several items are selected; the pane shows a summary instead.
    MultipleSelected,

    /// A folder: counts and the cached size, never a walk.
    Folder,

    /// Bigger than we are willing to read whole.
    TooLarge,

    /// A cloud placeholder whose content is not on this machine. Reading it would trigger a
    /// download, which is never something a preview may do on its own.
    NotDownloaded,

    /// Nothing could produce a preview. Produced at runtime, when the shell declines, rather than
    /// by the classifier, which cannot know what handlers are installed.
    NoPreview,
}

/// What the classifier is given: everything decidable without touching the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewTarget {
    pub name: String,
    pub size_bytes: i64,
    /// The Win32 attribute mask.
    pub attributes: u32,
    pub is_directory: bool,
}

/// The classifier's answer: how to read the file, or why not to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRequest {
    /// What to show. `PreviewKind::None` when `refusal` is set.
    pub kind: PreviewKind,
    /// Why not, or `PreviewRefusal::None`.
    pub refusal: PreviewRefusal,
    /// The most that may be read from the file. Zero when nothing is read from it directly
    /// (media and documents go through the shell, which streams its own way).
    pub byte_budget: i64,
    /// Which syntax table applies, for `PreviewKind::Text`.
    pub language: SyntaxLanguage,
    /// What the user asked for. Carried on the plan rather than read from the view model by the
    /// background build, so a plan cannot be executed under a mode it was not made for.
    pub mode: PreviewMode,
}

impl PreviewRequest {
    pub fn is_refused(&self) -> bool {
        self.refusal != PreviewRefusal::None
    }
}
