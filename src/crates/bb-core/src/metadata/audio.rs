//! Audio and video tags, read and written by lofty. **The only module that references it**,
//! which is what keeps the dependency replaceable — the same arrangement the archive reader has
//! with its library. Spec: Core/Services/Metadata/AudioTagCodec.cs (where ATL did the work).
//!
//! The library edits a file where it stands, so `write` copies the source into the destination
//! first and lets it edit the copy. The original is never opened for writing by anything; whether
//! the copy is fit to replace it is the executor's decision, made on `payload_digest`, which does
//! not ask the library anything at all. WMA is the one container the library has no reader for,
//! so its tags go through the in-house `asf` module instead, on the same terms.

use std::io::{self, Read, Seek, SeekFrom, Write};

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{FileType, TaggedFile};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagType};

use super::asf::{self, AsfTags};
use super::audio_payload::{AudioPayload, EXTENSIONS, rewind};
use super::codec::{
    Destination, MetadataCodec, MetadataError, MetadataResult, Source, copy_to_end,
};
use super::fields::{MetadataFamily, MetadataField};
use super::models::{MetadataDocument, MetadataEdit};
use crate::cancel::CancelToken;

const EVERYTHING: [MetadataField; 17] = [
    MetadataField::Title,
    MetadataField::Artist,
    MetadataField::AlbumArtist,
    MetadataField::Album,
    MetadataField::TrackNumber,
    MetadataField::TrackTotal,
    MetadataField::DiscNumber,
    MetadataField::DiscTotal,
    MetadataField::Year,
    MetadataField::Genre,
    MetadataField::Composer,
    MetadataField::Comment,
    MetadataField::Copyright,
    MetadataField::Publisher,
    MetadataField::Conductor,
    MetadataField::Bpm,
    MetadataField::Lyrics,
];

/// What each container has no place for. Found by writing every field to a real file of each
/// kind and reading it back, and pinned by the test that still does: a field offered here that a
/// format silently drops would fail the executor's read-back on every file of that kind.
fn missing(extension: &str) -> &'static [MetadataField] {
    match extension {
        ".wma" => &[MetadataField::Publisher],
        _ => &[],
    }
}

pub struct AudioTagCodec {
    extension: String,
    fields: Vec<MetadataField>,
}

impl AudioTagCodec {
    /// `extension` lowercase with its dot, one of `audio_payload::EXTENSIONS`.
    pub fn new(extension: &str) -> AudioTagCodec {
        let extension = extension.to_ascii_lowercase();
        let missing = missing(&extension);
        AudioTagCodec {
            fields: EVERYTHING
                .iter()
                .copied()
                .filter(|f| !missing.contains(f))
                .collect(),
            extension,
        }
    }

    pub fn extension(&self) -> &str {
        &self.extension
    }

    pub fn handles(extension: &str) -> bool {
        EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(extension))
    }

    fn file_type(&self) -> FileType {
        match self.extension.as_str() {
            ".mp3" => FileType::Mpeg,
            ".flac" => FileType::Flac,
            ".wav" => FileType::Wav,
            ".aiff" | ".aif" => FileType::Aiff,
            ".opus" => FileType::Opus,
            ".ogg" | ".oga" => FileType::Vorbis,
            _ => FileType::Mp4,
        }
    }

    fn takes(&self, field: MetadataField) -> bool {
        self.fields.contains(&field)
    }
}

fn not_audio() -> MetadataError {
    MetadataError::format("This is not a readable audio file, so it was left as it is.")
}

fn text(
    values: &mut std::collections::BTreeMap<MetadataField, String>,
    codec: &AudioTagCodec,
    field: MetadataField,
    value: Option<&str>,
) {
    if let Some(value) = value
        && codec.takes(field)
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            values.insert(field, trimmed.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }
}

fn number(
    values: &mut std::collections::BTreeMap<MetadataField, String>,
    codec: &AudioTagCodec,
    field: MetadataField,
    value: Option<u32>,
) {
    if let Some(value) = value
        && value > 0
        && codec.takes(field)
    {
        values.insert(field, value.to_string());
    }
}

/// A `Destination` as the library wants it: readable, writable, seekable, and able to say its
/// length and be cut short.
struct Edited<'a>(&'a mut dyn Destination);

impl Read for Edited<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Edited<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Seek for Edited<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl lofty::io::Truncate for Edited<'_> {
    fn truncate(&mut self, new_len: u64) -> io::Result<()> {
        self.0.truncate(new_len)
    }
}

impl lofty::io::Length for Edited<'_> {
    fn len(&self) -> io::Result<u64> {
        self.0.len()
    }
}

fn open(reader: &mut dyn Source, file_type: FileType) -> MetadataResult<TaggedFile> {
    // Whatever a third-party parser makes of a damaged file, it is one answer to us.
    reader.seek(SeekFrom::Start(0))?;
    Probe::with_file_type(reader, file_type)
        .options(ParseOptions::new().read_properties(false))
        .read()
        .map_err(|_| not_audio())
}

/// The key a container keeps lyrics, tempo and publisher under: the library spells them
/// differently per tag format and converts none of them.
fn lyrics_key(tag_type: TagType) -> ItemKey {
    match tag_type {
        TagType::Id3v2 => ItemKey::UnsyncLyrics,
        _ => ItemKey::Lyrics,
    }
}

fn bpm_key(tag_type: TagType) -> ItemKey {
    match tag_type {
        TagType::VorbisComments | TagType::Ape => ItemKey::Bpm,
        _ => ItemKey::IntegerBpm,
    }
}

fn publisher_key(tag_type: TagType) -> ItemKey {
    match tag_type {
        TagType::Mp4Ilst => ItemKey::Label,
        _ => ItemKey::Publisher,
    }
}

fn first_string<'t>(tag: &'t Tag, keys: &[ItemKey]) -> Option<&'t str> {
    keys.iter().find_map(|k| tag.get_string(*k))
}

fn mime_of(picture: &[u8]) -> MimeType {
    if picture.starts_with(&[0x89, b'P', b'N', b'G']) {
        MimeType::Png
    } else if picture.starts_with(&[0xFF, 0xD8]) {
        MimeType::Jpeg
    } else {
        MimeType::Unknown("application/octet-stream".to_string())
    }
}

impl AudioTagCodec {
    fn read_tagged(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        let file = open(source, self.file_type())?;
        let mut values = std::collections::BTreeMap::new();
        let mut picture = None;

        if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
            let tag_type = tag.tag_type();
            text(
                &mut values,
                self,
                MetadataField::Title,
                tag.title().as_deref(),
            );
            text(
                &mut values,
                self,
                MetadataField::Artist,
                tag.artist().as_deref(),
            );
            text(
                &mut values,
                self,
                MetadataField::AlbumArtist,
                tag.get_string(ItemKey::AlbumArtist),
            );
            text(
                &mut values,
                self,
                MetadataField::Album,
                tag.album().as_deref(),
            );
            text(
                &mut values,
                self,
                MetadataField::Genre,
                tag.genre().as_deref(),
            );
            text(
                &mut values,
                self,
                MetadataField::Composer,
                tag.get_string(ItemKey::Composer),
            );
            text(
                &mut values,
                self,
                MetadataField::Conductor,
                tag.get_string(ItemKey::Conductor),
            );
            text(
                &mut values,
                self,
                MetadataField::Publisher,
                first_string(
                    tag,
                    &[publisher_key(tag_type), ItemKey::Publisher, ItemKey::Label],
                ),
            );
            text(
                &mut values,
                self,
                MetadataField::Comment,
                tag.comment().as_deref(),
            );
            text(
                &mut values,
                self,
                MetadataField::Copyright,
                tag.get_string(ItemKey::CopyrightMessage),
            );
            text(
                &mut values,
                self,
                MetadataField::Lyrics,
                first_string(tag, &[ItemKey::UnsyncLyrics, ItemKey::Lyrics]),
            );
            number(&mut values, self, MetadataField::TrackNumber, tag.track());
            number(
                &mut values,
                self,
                MetadataField::TrackTotal,
                tag.track_total(),
            );
            number(&mut values, self, MetadataField::DiscNumber, tag.disk());
            number(
                &mut values,
                self,
                MetadataField::DiscTotal,
                tag.disk_total(),
            );
            number(&mut values, self, MetadataField::Year, year_of(tag));
            let bpm = first_string(tag, &[ItemKey::IntegerBpm, ItemKey::Bpm])
                .and_then(|b| b.trim().parse::<f64>().ok())
                .map(|b| b.round() as u32);
            number(&mut values, self, MetadataField::Bpm, bpm);

            // The front cover when one is marked as such, else whatever picture comes first.
            picture = tag
                .pictures()
                .iter()
                .find(|p| p.pic_type() == PictureType::CoverFront)
                .or_else(|| tag.pictures().first())
                .map(|p| p.data().to_vec())
                .filter(|d| !d.is_empty());
        }

        let mut document = MetadataDocument::new(MetadataFamily::Audio, values);
        document.picture = picture;
        Ok(document)
    }

    fn read_asf(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        let tags = asf::read(source)?;
        let mut values = std::collections::BTreeMap::new();
        for (field, value) in tags.fields() {
            if self.takes(field) {
                values.insert(field, value);
            }
        }
        let mut document = MetadataDocument::new(MetadataFamily::Audio, values);
        document.picture = tags.picture.filter(|p| !p.is_empty());
        Ok(document)
    }

    fn write_tagged(
        &self,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()> {
        let mut edited = Edited(destination);
        edited.seek(SeekFrom::Start(0))?;
        let mut file = open(&mut edited, self.file_type())?;

        if edit.remove_all {
            // Every tag the file carries, of every kind — an MP3 can have three at once.
            let kinds: Vec<TagType> = file.tags().iter().map(|t| t.tag_type()).collect();
            for kind in kinds {
                edited.seek(SeekFrom::Start(0))?;
                kind.remove_from(&mut edited, WriteOptions::default())
                    .map_err(|_| {
                        MetadataError::format(
                            "The tags could not be removed, so the file was left as it is.",
                        )
                    })?;
            }

            edited.flush()?;
            if edit.changes.is_empty() && edit.set_picture.is_none() {
                return Ok(());
            }

            file = open(&mut edited, self.file_type())?;
        }

        let tag_type = file.primary_tag_type();
        if file.tag(tag_type).is_none() {
            file.insert_tag(Tag::new(tag_type));
        }
        let tag = file.tag_mut(tag_type).expect("just inserted");

        for (field, value) in &edit.changes {
            let parsed: u32 = value.parse().unwrap_or(0);
            let set_text = |tag: &mut Tag, key: ItemKey| {
                if value.is_empty() {
                    tag.remove_key(key);
                } else {
                    tag.insert_text(key, value.clone());
                }
            };
            match field {
                MetadataField::Title => {
                    if value.is_empty() {
                        tag.remove_title()
                    } else {
                        tag.set_title(value.clone())
                    }
                }
                MetadataField::Artist => {
                    if value.is_empty() {
                        tag.remove_artist()
                    } else {
                        tag.set_artist(value.clone())
                    }
                }
                MetadataField::AlbumArtist => set_text(tag, ItemKey::AlbumArtist),
                MetadataField::Album => {
                    if value.is_empty() {
                        tag.remove_album()
                    } else {
                        tag.set_album(value.clone())
                    }
                }
                MetadataField::Genre => {
                    if value.is_empty() {
                        tag.remove_genre()
                    } else {
                        tag.set_genre(value.clone())
                    }
                }
                MetadataField::Composer => set_text(tag, ItemKey::Composer),
                MetadataField::Conductor => set_text(tag, ItemKey::Conductor),
                MetadataField::Publisher => {
                    tag.remove_key(ItemKey::Publisher);
                    tag.remove_key(ItemKey::Label);
                    set_text(tag, publisher_key(tag_type));
                }
                MetadataField::Comment => {
                    if value.is_empty() {
                        tag.remove_comment()
                    } else {
                        tag.set_comment(value.clone())
                    }
                }
                MetadataField::Copyright => set_text(tag, ItemKey::CopyrightMessage),
                MetadataField::TrackNumber => {
                    if parsed == 0 {
                        tag.remove_track()
                    } else {
                        tag.set_track(parsed)
                    }
                }
                MetadataField::TrackTotal => {
                    if parsed == 0 {
                        tag.remove_track_total()
                    } else {
                        tag.set_track_total(parsed)
                    }
                }
                MetadataField::DiscNumber => {
                    if parsed == 0 {
                        tag.remove_disk()
                    } else {
                        tag.set_disk(parsed)
                    }
                }
                MetadataField::DiscTotal => {
                    if parsed == 0 {
                        tag.remove_disk_total()
                    } else {
                        tag.set_disk_total(parsed)
                    }
                }
                MetadataField::Year => {
                    if parsed == 0 {
                        tag.remove_key(ItemKey::RecordingDate);
                        tag.remove_key(ItemKey::Year);
                    } else {
                        tag.remove_key(ItemKey::RecordingDate);
                        tag.remove_key(ItemKey::Year);
                        tag.insert_text(year_key(tag_type), parsed.to_string());
                    }
                }
                MetadataField::Bpm => {
                    tag.remove_key(ItemKey::Bpm);
                    tag.remove_key(ItemKey::IntegerBpm);
                    if parsed > 0 {
                        tag.insert_text(bpm_key(tag_type), parsed.to_string());
                    }
                }
                MetadataField::Lyrics => {
                    tag.remove_key(ItemKey::Lyrics);
                    tag.remove_key(ItemKey::UnsyncLyrics);
                    if !value.is_empty() {
                        tag.insert_text(lyrics_key(tag_type), value.clone());
                    }
                }
                _ => {}
            }
        }

        if edit.remove_picture || edit.set_picture.is_some() {
            while tag.picture_count() > 0 {
                tag.remove_picture(0);
            }
            if let Some(picture) = &edit.set_picture {
                tag.push_picture(
                    Picture::unchecked(picture.clone())
                        .pic_type(PictureType::CoverFront)
                        .mime_type(mime_of(picture))
                        .build(),
                );
            }
        }

        // Every one of these formats stores a total as the second half of "3/12". With no first
        // half there is nowhere to put it, and it would be dropped without a word.
        if tag.track_total().is_some_and(|t| t > 0) && !tag.track().is_some_and(|n| n > 0) {
            return Err(MetadataError::format(
                "A number of tracks needs a track number to go with it.",
            ));
        }
        if tag.disk_total().is_some_and(|t| t > 0) && !tag.disk().is_some_and(|n| n > 0) {
            return Err(MetadataError::format(
                "A number of discs needs a disc number to go with it.",
            ));
        }

        cancel.check()?;
        edited.seek(SeekFrom::Start(0))?;
        file.save_to(&mut edited, WriteOptions::default())
            .map_err(|_| {
                MetadataError::format(
                    "The tags could not be written, so the file was left as it is.",
                )
            })?;
        edited.flush()?;
        Ok(())
    }

    fn write_asf(
        &self,
        source: &mut dyn Source,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()> {
        let mut tags = if edit.remove_all {
            AsfTags::default()
        } else {
            asf::read(source)?
        };

        for (field, value) in &edit.changes {
            tags.apply(*field, value)?;
        }

        if edit.remove_picture || edit.set_picture.is_some() {
            tags.picture = edit.set_picture.clone();
        }

        if tags.orphaned_total("WM/TrackNumber") {
            return Err(MetadataError::format(
                "A number of tracks needs a track number to go with it.",
            ));
        }
        if tags.orphaned_total("WM/PartOfSet") {
            return Err(MetadataError::format(
                "A number of discs needs a disc number to go with it.",
            ));
        }

        asf::rewrite(source, destination, &tags, cancel)
    }
}

impl MetadataCodec for AudioTagCodec {
    fn id(&self) -> &'static str {
        "audio"
    }

    fn family(&self) -> MetadataFamily {
        MetadataFamily::Audio
    }

    fn fields(&self) -> &[MetadataField] {
        &self.fields
    }

    fn holds_picture(&self) -> bool {
        true
    }

    fn read(&self, source: &mut dyn Source) -> MetadataResult<MetadataDocument> {
        // Ours first: it is what decides whether this is the format its name claims.
        AudioPayload::digest(&self.extension, source)?;
        rewind(source)?;

        if self.extension == ".wma" {
            self.read_asf(source)
        } else {
            self.read_tagged(source)
        }
    }

    fn write(
        &self,
        source: &mut dyn Source,
        destination: &mut dyn Destination,
        edit: &MetadataEdit,
        cancel: &CancelToken,
    ) -> MetadataResult<()> {
        AudioPayload::digest(&self.extension, source)?;

        if self.extension == ".wma" {
            return self.write_asf(source, destination, edit, cancel);
        }

        copy_to_end(source, destination, 0, cancel)?;
        destination.flush()?;
        self.write_tagged(destination, edit, cancel)
    }

    fn payload_digest(&self, source: &mut dyn Source) -> MetadataResult<Vec<u8>> {
        AudioPayload::digest(&self.extension, source)
    }
}

/// The four-digit year of whichever date item the tag keeps (a full recording date or a bare year).
fn year_of(tag: &Tag) -> Option<u32> {
    let text = first_string(tag, &[ItemKey::RecordingDate, ItemKey::Year])?;
    let digits: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok().filter(|y| *y > 0)
}

/// Where a year is kept: its own item where the format has one, the recording date (which a year
/// alone is a valid value of) where it does not - ID3v2, MP4 and RIFF INFO.
fn year_key(tag_type: TagType) -> ItemKey {
    if ItemKey::Year.map_key(tag_type).is_some() {
        ItemKey::Year
    } else {
        ItemKey::RecordingDate
    }
}
