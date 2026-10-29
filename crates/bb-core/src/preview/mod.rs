//! The preview pane's rules: what a selection gets (the classifier), the text and hex readers
//! over a file's front, the syntax colouring, the details strip, the media playlist, the image
//! zoom arithmetic, the tile aspect ratio, and the GIF animation pieces. Everything here is pure
//! over bytes or names; opening a file is the caller's, through `fs::ReadOnlyFile`.
//! Spec: src/BertBrowser.Core/Services/Preview/*, Core/Models/AspectRatio.cs.

pub mod animation;
mod aspect_ratio;
mod classifier;
mod hex_reader;
mod kind;
mod metadata;
mod playlist;
mod syntax;
mod text_reader;
mod zoom;

pub use aspect_ratio::AspectRatio;
pub use classifier::PreviewClassifier;
pub use hex_reader::{HexPreview, HexPreviewReader, HexRow};
pub use kind::{PreviewKind, PreviewMode, PreviewRefusal, PreviewRequest, PreviewTarget};
pub use metadata::{
    DOCUMENT_ORDER, IMAGE_ORDER, MEDIA_ORDER, MetadataRow, PreviewMetadata, ShellPropertyRow,
    shorten_canonical,
};
pub use playlist::MediaPlaylist;
pub use syntax::{SyntaxClass, SyntaxLanguage, SyntaxSpan, SyntaxTokenizer};
pub use text_reader::{TextPreview, TextPreviewReader, TextReadOptions};
pub use zoom::ImageZoom;
