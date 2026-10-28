//! Which codec handles a file. The extension decides, never a sniff of the bytes: a codec that
//! then finds something else inside refuses it by name.
//! Spec: Core/Services/Metadata/IMetadataCodec.cs (`MetadataCodecs`).

use super::audio::AudioTagCodec;
use super::audio_payload::EXTENSIONS as AUDIO_EXTENSIONS;
use super::codec::MetadataCodec;
use super::epub::EpubCodec;
use super::jpeg::JpegCodec;
use super::openxml::OpenXmlCodec;
use super::png::PngCodec;
use super::tiff::TiffCodec;
use super::webp::WebPCodec;

const PICTURES: [&str; 8] = [
    ".jpg", ".jpeg", ".jpe", ".jfif", ".png", ".tif", ".tiff", ".webp",
];

/// Word, Excel and PowerPoint, with their macro-enabled and template variants: one container,
/// one properties part.
const OPEN_XML: [&str; 14] = [
    ".docx", ".docm", ".dotx", ".dotm", ".xlsx", ".xlsm", ".xltx", ".xltm", ".pptx", ".pptm",
    ".potx", ".potm", ".ppsx", ".ppsm",
];

pub struct MetadataCodecs;

impl MetadataCodecs {
    /// The extensions something here can write, lowercase with their dot.
    pub fn extensions() -> Vec<&'static str> {
        let mut all: Vec<&'static str> = PICTURES.to_vec();
        all.push(".epub");
        all.extend(OPEN_XML);
        // Exactly the containers the payload check knows how to read — no more, whatever the
        // tag library could manage on its own.
        all.extend(AUDIO_EXTENSIONS);
        all
    }

    pub fn handles(path: &str) -> bool {
        Self::for_path(path).is_some()
    }

    pub fn for_path(path: &str) -> Option<Box<dyn MetadataCodec>> {
        let extension = extension_of(path).to_ascii_lowercase();
        Self::for_extension(&extension)
    }

    /// The codec for an extension (lowercase, with its dot), or None.
    pub fn for_extension(extension: &str) -> Option<Box<dyn MetadataCodec>> {
        Some(match extension {
            ".jpg" | ".jpeg" | ".jpe" | ".jfif" => Box::new(JpegCodec),
            ".png" => Box::new(PngCodec),
            ".tif" | ".tiff" => Box::new(TiffCodec),
            ".webp" => Box::new(WebPCodec),
            ".epub" => Box::new(EpubCodec),
            e if OPEN_XML.contains(&e) => Box::new(OpenXmlCodec),
            e if AUDIO_EXTENSIONS.contains(&e) => Box::new(AudioTagCodec::new(e)),
            _ => return None,
        })
    }
}

/// `Path.GetExtension`: the last dot of the last segment and what follows it, or nothing.
pub(crate) fn extension_of(path: &str) -> &str {
    let name = bb_paths::file_name(path);
    match name.rfind('.') {
        Some(dot) => &name[dot..],
        None => "",
    }
}
