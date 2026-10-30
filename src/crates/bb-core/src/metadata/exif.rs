//! An EXIF block — a small TIFF file carried inside a picture — that can be edited without moving
//! anything already in it. Spec: Core/Services/Metadata/ExifBlock.cs.
//!
//! **Nothing that exists is relocated.** A camera's MakerNote is an opaque blob full of offsets
//! measured from the start of this block, and nothing outside the manufacturer knows which bytes
//! they are. A writer that re-serialises the block tidily moves the blob and silently breaks every
//! one of them. So a changed value is written over its own old bytes when it fits and appended to
//! the end when it does not; a directory that grew is appended whole and the one pointer to it is
//! redirected. The cost is a few dead bytes per edit, and that is the whole cost.
//!
//! **Dead bytes are zeroed**, because a removed value that is still sitting in the file has not
//! been removed — which matters the day the value is where a photograph was taken. The exception
//! is a range something else also points into (some writers share one copy of a repeated value):
//! that is left alone and never reused, since the other owner still needs it.
//!
//! Structure this has to write into is parsed strictly and a damaged one is refused. A value it
//! merely carries is parsed leniently: an entry pointing off the end of the block is kept
//! byte-for-byte and reads as absent.

use std::collections::HashSet;

use super::codec::{MetadataError, MetadataResult};

/// Which directory of an EXIF block a tag lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExifDirectory {
    /// IFD0: the picture's own description, and the Windows `XP*` tags.
    Image,
    /// The Exif sub-directory: how and when it was taken.
    Photo,
}

pub const TYPE_BYTE: u16 = 1;
pub const TYPE_ASCII: u16 = 2;
pub const TYPE_SHORT: u16 = 3;
pub const TYPE_LONG: u16 = 4;
pub const TYPE_RATIONAL: u16 = 5;
pub const TYPE_UNDEFINED: u16 = 7;

const EXIF_POINTER: u16 = 0x8769;
const GPS_POINTER: u16 = 0x8825;
const INTEROP_POINTER: u16 = 0xA005;
const THUMBNAIL_OFFSET: u16 = 0x0201;
const THUMBNAIL_LENGTH: u16 = 0x0202;

/// The most a JPEG APP1 segment can carry after its `Exif\0\0` label.
pub const MAX_LENGTH: usize = 65_527;

/// Who a claimed byte range belongs to: an entry's value, or a directory's table.
type Owner = u32;

#[derive(Clone, Copy, Debug)]
struct Claim {
    offset: usize,
    length: usize,
    owner: Owner,
}

#[derive(Clone, Debug)]
struct Entry {
    id: Owner,
    tag: u16,
    kind: u16,
    count: u32,
    /// The four value bytes as stored: the value itself, or the offset of it.
    inline: [u8; 4],
    /// Where the value is when it does not fit inline.
    data_offset: Option<usize>,
    data_length: usize,
    replacement: Option<Vec<u8>>,
    /// Carried byte-for-byte and never interpreted: an unknown type, or a value that points off
    /// the end of the block.
    opaque: bool,
}

#[derive(Clone, Debug)]
struct Directory {
    id: Owner,
    /// Where the table is, or None for one that has not been written yet.
    offset: Option<usize>,
    table_length: usize,
    next: u32,
    dirty: bool,
    entries: Vec<Entry>,
}

impl Directory {
    fn find(&self, tag: u16) -> Option<&Entry> {
        self.entries.iter().find(|e| e.tag == tag)
    }

    fn find_mut(&mut self, tag: u16) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.tag == tag)
    }
}

pub struct ExifBlock {
    original: Vec<u8>,
    little: bool,
    image: Directory,
    photo: Option<Directory>,
    gps: Option<Directory>,
    /// Every byte range a parsed entry or directory occupies, so a range two things share can be
    /// told from one that is safe to overwrite.
    claims: Vec<Claim>,
    abandoned: Vec<Claim>,
    next_owner: Owner,
}

impl ExifBlock {
    fn damaged() -> MetadataError {
        MetadataError::format("This picture's EXIF block is damaged, so it was left as it is.")
    }

    /// A block with nothing in it, for a picture that has none.
    pub fn empty() -> ExifBlock {
        ExifBlock {
            original: vec![b'I', b'I', 42, 0, 0, 0, 0, 0],
            little: true,
            image: Directory {
                id: 0,
                offset: None,
                table_length: 0,
                next: 0,
                dirty: false,
                entries: Vec::new(),
            },
            photo: None,
            gps: None,
            claims: Vec::new(),
            abandoned: Vec::new(),
            next_owner: 1,
        }
    }

    /// `Format` when not a TIFF structure, or damaged where it would have to be written.
    pub fn parse(tiff: &[u8]) -> MetadataResult<ExifBlock> {
        if tiff.len() < 8 {
            return Err(Self::damaged());
        }
        let little = match (tiff[0], tiff[1]) {
            (b'I', b'I') => true,
            (b'M', b'M') => false,
            _ => return Err(Self::damaged()),
        };
        if u16_at(tiff, 2, little) != 42 {
            return Err(Self::damaged());
        }

        let mut block = ExifBlock::empty();
        block.original = tiff.to_vec();
        block.little = little;
        let mut seen = HashSet::new();

        let image = block
            .read_directory(u32_at(tiff, 4, little) as usize, &mut seen)
            .ok_or_else(Self::damaged)?;
        block.image = image;

        if let Some(photo_at) = block.pointer_of(&block.image, EXIF_POINTER) {
            let photo = block
                .read_directory(photo_at, &mut seen)
                .ok_or_else(Self::damaged)?;
            block.photo = Some(photo);
        }

        // The GPS directory is never written to, only removed whole; the rest are walked so
        // their bytes are known to be in use.
        if let Some(gps_at) = block.pointer_of(&block.image, GPS_POINTER) {
            block.gps = block.read_directory(gps_at, &mut seen);
        }
        if let Some(interop_at) = block
            .photo
            .as_ref()
            .and_then(|photo| block.pointer_of(photo, INTEROP_POINTER))
        {
            block.read_directory(interop_at, &mut seen);
        }

        let mut next = block.image.next;
        while next != 0 {
            let Some(thumbnail) = block.read_directory(next as usize, &mut seen) else {
                break;
            };
            if let (Some(at), Some(length)) = (
                block.pointer_of(&thumbnail, THUMBNAIL_OFFSET),
                block.pointer_of(&thumbnail, THUMBNAIL_LENGTH),
            ) && length > 0
                && at + length <= block.original.len()
            {
                block.claims.push(Claim {
                    offset: at,
                    length,
                    owner: thumbnail.id,
                });
            }
            next = thumbnail.next;
        }

        Ok(block)
    }

    /// The raw bytes of a tag's value, or None when it is absent or unreadable.
    pub fn get(&self, directory: ExifDirectory, tag: u16) -> Option<Vec<u8>> {
        let entry = self.directory_of(directory)?.find(tag)?;
        if let Some(replaced) = &entry.replacement {
            return Some(replaced.clone());
        }
        if entry.opaque {
            return None;
        }
        Some(match entry.data_offset {
            Some(at) => self.original[at..at + entry.data_length].to_vec(),
            None => entry.inline[..entry.data_length].to_vec(),
        })
    }

    pub fn type_of(&self, directory: ExifDirectory, tag: u16) -> Option<u16> {
        Some(self.directory_of(directory)?.find(tag)?.kind)
    }

    pub fn set(&mut self, directory: ExifDirectory, tag: u16, kind: u16, value: Vec<u8>) {
        let size = size_of(kind);
        assert!(
            size != 0 && !value.is_empty() && value.len().is_multiple_of(size),
            "not a value of that type"
        );

        let count = (value.len() / size) as u32;
        let next_owner = &mut self.next_owner;
        let target = match directory {
            ExifDirectory::Image => &mut self.image,
            ExifDirectory::Photo => self.photo.get_or_insert_with(|| {
                let id = *next_owner;
                *next_owner += 1;
                Directory {
                    id,
                    offset: None,
                    table_length: 0,
                    next: 0,
                    dirty: false,
                    entries: Vec::new(),
                }
            }),
        };

        if target.find(tag).is_none() {
            let id = *next_owner;
            *next_owner += 1;
            target.entries.push(Entry {
                id,
                tag,
                kind,
                count,
                inline: [0; 4],
                data_offset: None,
                data_length: 0,
                replacement: None,
                opaque: false,
            });
        }
        let entry = target.find_mut(tag).expect("just added");
        entry.kind = kind;
        entry.count = count;
        entry.replacement = Some(value);
        entry.opaque = false;
        target.dirty = true;
    }

    pub fn remove(&mut self, directory: ExifDirectory, tag: u16) {
        let Some(target) = self.directory_of_mut(directory) else {
            return;
        };
        let Some(index) = target.entries.iter().position(|e| e.tag == tag) else {
            return;
        };
        let entry = target.entries.remove(index);
        target.dirty = true;
        if let Some(at) = entry.data_offset {
            self.abandoned.push(Claim {
                offset: at,
                length: entry.data_length,
                owner: entry.id,
            });
        }
    }

    /// Whether the block says where the picture was taken. True for a GPS pointer this could not
    /// follow, too: an unreadable location is still one, and "none here" must not be the answer.
    pub fn has_location(&self) -> bool {
        self.gps.is_some() || self.image.find(GPS_POINTER).is_some()
    }

    /// Takes the whole Exif sub-directory out — how and when the picture was taken, the maker's
    /// note, serial numbers — zeroing every value it held. For a format where the block cannot
    /// simply be dropped, because the picture itself lives in it.
    pub fn remove_photo_directory(&mut self) {
        let Some(photo) = self.photo.take() else {
            self.remove(ExifDirectory::Image, EXIF_POINTER);
            return;
        };

        for entry in &photo.entries {
            if let Some(at) = entry.data_offset {
                self.abandoned.push(Claim {
                    offset: at,
                    length: entry.data_length,
                    owner: entry.id,
                });
            }
        }
        if let Some(at) = photo.offset {
            self.abandoned.push(Claim {
                offset: at,
                length: photo.table_length,
                owner: photo.id,
            });
        }
        self.remove(ExifDirectory::Image, EXIF_POINTER);
    }

    /// Takes the GPS directory out: the pointer to it, the table, and every value it held,
    /// zeroed. `Format` when the directory is damaged, so its bytes cannot be found to erase —
    /// and unlinking it while leaving them would only look like removal.
    pub fn remove_location(&mut self) -> MetadataResult<()> {
        if !self.has_location() {
            return Ok(());
        }

        let unsafe_to_erase = || {
            MetadataError::format(
                "This picture's location is stored in a damaged block that cannot be safely erased.",
            )
        };
        let Some(gps) = self.gps.as_ref() else {
            return Err(unsafe_to_erase());
        };
        if gps.entries.iter().any(|e| e.opaque) {
            return Err(unsafe_to_erase());
        }

        let gps = self.gps.take().expect("checked above");
        for entry in &gps.entries {
            if let Some(at) = entry.data_offset {
                self.abandoned.push(Claim {
                    offset: at,
                    length: entry.data_length,
                    owner: entry.id,
                });
            }
        }
        if let Some(at) = gps.offset {
            self.abandoned.push(Claim {
                offset: at,
                length: gps.table_length,
                owner: gps.id,
            });
        }
        self.remove(ExifDirectory::Image, GPS_POINTER);
        Ok(())
    }

    pub fn is_little_endian(&self) -> bool {
        self.little
    }

    pub fn read_u16(&self, value: &[u8]) -> u16 {
        u16_at(value, 0, self.little)
    }

    pub fn read_u32(&self, value: &[u8]) -> u32 {
        u32_at(value, 0, self.little)
    }

    pub fn write_u16(&self, value: u16) -> Vec<u8> {
        if self.little {
            value.to_le_bytes().to_vec()
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    pub fn write_u32(&self, value: u32) -> Vec<u8> {
        if self.little {
            value.to_le_bytes().to_vec()
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    /// The block with every change applied. `max_length` is the most the result may be: a JPEG
    /// segment's limit, or none of its own for a TIFF file, which is one of these blocks from end
    /// to end. `Format` when the result would not fit.
    pub fn to_bytes(&mut self, max_length: usize) -> MetadataResult<Vec<u8>> {
        let mut buffer = self.original.clone();

        let abandoned = std::mem::take(&mut self.abandoned);
        for claim in &abandoned {
            self.zero(&mut buffer, claim.offset, claim.length, claim.owner);
        }

        if let Some(mut photo) = self.photo.take() {
            let moved = self.commit(&mut buffer, &mut photo);
            self.photo = Some(photo);
            if let Some(at) = moved {
                let pointer = self.write_u32(at as u32);
                self.set(ExifDirectory::Image, EXIF_POINTER, TYPE_LONG, pointer);
            }
        }

        let mut image = std::mem::replace(&mut self.image, ExifBlock::empty().image);
        let moved = self.commit(&mut buffer, &mut image);
        self.image = image;
        if let Some(at) = moved {
            let pointer = self.write_u32(at as u32);
            buffer[4..8].copy_from_slice(&pointer);
        }

        if buffer.len() > max_length {
            return Err(MetadataError::format(
                "There is no room left in this picture's EXIF block.",
            ));
        }
        Ok(buffer)
    }

    /// Writes a directory's changed values and its table, and says where the table went when it
    /// had to move.
    fn commit(&mut self, buffer: &mut Vec<u8>, directory: &mut Directory) -> Option<usize> {
        if !directory.dirty && directory.offset.is_some() {
            return None;
        }

        for entry in &mut directory.entries {
            let Some(value) = entry.replacement.take() else {
                continue;
            };

            if value.len() <= 4 {
                if let Some(at) = entry.data_offset {
                    self.zero(buffer, at, entry.data_length, entry.id);
                }
                entry.inline = [0; 4];
                entry.inline[..value.len()].copy_from_slice(&value);
                entry.data_offset = None;
            } else if let Some(at) = entry.data_offset.filter(|&at| {
                entry.data_length >= value.len() && !self.shared(at, entry.data_length, entry.id)
            }) {
                self.zero(buffer, at, entry.data_length, entry.id);
                buffer[at..at + value.len()].copy_from_slice(&value);
                entry.inline.copy_from_slice(&self.write_u32(at as u32));
            } else {
                if let Some(at) = entry.data_offset {
                    self.zero(buffer, at, entry.data_length, entry.id);
                }
                let at = append(buffer, &value);
                entry.data_offset = Some(at);
                entry.inline.copy_from_slice(&self.write_u32(at as u32));
            }

            entry.data_length = value.len();
        }

        directory.entries.sort_by_key(|e| e.tag);
        let mut table = vec![0u8; 2 + 12 * directory.entries.len() + 4];
        table[..2].copy_from_slice(&self.write_u16(directory.entries.len() as u16));
        for (i, entry) in directory.entries.iter().enumerate() {
            let at = 2 + 12 * i;
            table[at..at + 2].copy_from_slice(&self.write_u16(entry.tag));
            table[at + 2..at + 4].copy_from_slice(&self.write_u16(entry.kind));
            table[at + 4..at + 8].copy_from_slice(&self.write_u32(entry.count));
            table[at + 8..at + 12].copy_from_slice(&entry.inline);
        }
        let end = table.len();
        table[end - 4..].copy_from_slice(&self.write_u32(directory.next));

        let room = directory.offset.map_or(0, |_| directory.table_length);
        let moved = match directory.offset {
            Some(at) if table.len() <= room && !self.shared(at, room, directory.id) => {
                self.zero(buffer, at, room, directory.id);
                buffer[at..at + table.len()].copy_from_slice(&table);
                None
            }
            _ => {
                if let Some(at) = directory.offset {
                    self.zero(buffer, at, room, directory.id);
                }
                let at = append(buffer, &table);
                directory.offset = Some(at);
                Some(at)
            }
        };

        directory.table_length = table.len();
        directory.dirty = false;
        moved
    }

    fn zero(&self, buffer: &mut [u8], offset: usize, length: usize, owner: Owner) {
        if self.shared(offset, length, owner) {
            return;
        }
        buffer[offset..offset + length].fill(0);
    }

    fn shared(&self, offset: usize, length: usize, owner: Owner) -> bool {
        self.claims.iter().any(|claim| {
            claim.owner != owner
                && claim.offset < offset + length
                && offset < claim.offset + claim.length
        })
    }

    fn directory_of(&self, directory: ExifDirectory) -> Option<&Directory> {
        match directory {
            ExifDirectory::Image => Some(&self.image),
            ExifDirectory::Photo => self.photo.as_ref(),
        }
    }

    fn directory_of_mut(&mut self, directory: ExifDirectory) -> Option<&mut Directory> {
        match directory {
            ExifDirectory::Image => Some(&mut self.image),
            ExifDirectory::Photo => self.photo.as_mut(),
        }
    }

    fn pointer_of(&self, directory: &Directory, tag: u16) -> Option<usize> {
        let entry = directory.find(tag)?;
        if entry.opaque || entry.data_offset.is_some() || entry.count != 1 {
            return None;
        }
        match entry.kind {
            TYPE_LONG => Some(u32_at(&entry.inline, 0, self.little) as usize),
            TYPE_SHORT => Some(u16_at(&entry.inline, 0, self.little) as usize),
            _ => None,
        }
    }

    /// Reads one directory, or returns None when it is not there to be read.
    fn read_directory(&mut self, offset: usize, seen: &mut HashSet<usize>) -> Option<Directory> {
        // A directory already walked is a loop, which a hostile file can make as long as it likes.
        if offset < 8 || !seen.insert(offset) {
            return None;
        }
        let data = &self.original;
        if offset + 2 > data.len() {
            return None;
        }

        let count = u16_at(data, offset, self.little) as usize;
        let length = 2 + 12 * count + 4;
        if offset + length > data.len() {
            return None;
        }

        let id = self.next_owner;
        self.next_owner += 1;
        let mut directory = Directory {
            id,
            offset: Some(offset),
            table_length: length,
            next: 0,
            dirty: false,
            entries: Vec::with_capacity(count),
        };
        let mut claims = Vec::new();

        for i in 0..count {
            let at = offset + 2 + 12 * i;
            let kind = u16_at(data, at + 2, self.little);
            let entry_count = u32_at(data, at + 4, self.little);
            let mut inline = [0u8; 4];
            inline.copy_from_slice(&data[at + 8..at + 12]);
            let entry_id = self.next_owner;
            self.next_owner += 1;
            let mut entry = Entry {
                id: entry_id,
                tag: u16_at(data, at, self.little),
                kind,
                count: entry_count,
                inline,
                data_offset: None,
                data_length: 0,
                replacement: None,
                opaque: false,
            };

            let size = size_of(kind) as u64 * entry_count as u64;
            if size == 0 {
                entry.opaque = true;
            } else if size <= 4 {
                entry.data_length = size as usize;
            } else {
                let at = u32_at(&inline, 0, self.little) as u64;
                if at + size > data.len() as u64 {
                    entry.opaque = true;
                } else {
                    entry.data_offset = Some(at as usize);
                    entry.data_length = size as usize;
                    claims.push(Claim {
                        offset: at as usize,
                        length: size as usize,
                        owner: entry_id,
                    });
                }
            }

            directory.entries.push(entry);
        }

        directory.next = u32_at(data, offset + length - 4, self.little);
        claims.push(Claim {
            offset,
            length,
            owner: id,
        });
        self.claims.extend(claims);
        Some(directory)
    }
}

/// Adds bytes at the end, on an even offset as TIFF asks, and says where.
fn append(buffer: &mut Vec<u8>, value: &[u8]) -> usize {
    if !buffer.len().is_multiple_of(2) {
        buffer.push(0);
    }
    let at = buffer.len();
    buffer.extend_from_slice(value);
    at
}

fn size_of(kind: u16) -> usize {
    match kind {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 0,
    }
}

pub(crate) fn u16_at(data: &[u8], at: usize, little: bool) -> u16 {
    let bytes = [data[at], data[at + 1]];
    if little {
        u16::from_le_bytes(bytes)
    } else {
        u16::from_be_bytes(bytes)
    }
}

pub(crate) fn u32_at(data: &[u8], at: usize, little: bool) -> u32 {
    let bytes = [data[at], data[at + 1], data[at + 2], data[at + 3]];
    if little {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    }
}
