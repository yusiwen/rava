//! Reading the host JDK's module image (`lib/modules`).
//!
//! ADR-0008 makes the image the class source: a JDK ships every class and resource inside
//! one file, indexed by a perfect hash so a name lookup never scans. The format is the one
//! `jdk.internal.jimage` reads, and its layout is small enough to restate here.
//!
//! All integers are **host-endian**: `jlink` writes the image for the machine it targets, so
//! a little-endian host reads little-endian words. Cross-endian images are not a thing.
//!
//! ```text
//! header      7 × u32: magic, version, flags, resource count, table length,
//!             locations size, strings size
//! redirect    table length × i32: 0 = empty, negative = index, positive = rehash seed
//! offsets     table length × u32: location index -> offset into the locations blob
//! locations   variable-length attribute records (module, parent, base, extension,
//!             content offset, compressed size, uncompressed size)
//! strings     NUL-terminated modified UTF-8; the three parts of a name are stored once
//! content     the resource bytes, at an offset measured from the end of the index
//! ```
//!
//! The reader is deliberately read-only and eager: it keeps the whole file in memory. An
//! image is 140 MB on a current JDK, which is the price of not owning a mapping abstraction
//! yet; the seam for a lazy reader is [`Image::resource`], not the callers.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// `0xCAFEDADA`, the first word of every image.
pub const MAGIC: u32 = 0xCAFE_DADA;

/// The only version this reader understands (`jlink` writes it as `1.0`).
const MAJOR_VERSION: u16 = 1;
const MINOR_VERSION: u16 = 0;

const HEADER_SLOTS: usize = 7;
const HEADER_SIZE: usize = HEADER_SLOTS * 4;

/// The hash `jimage` uses for the redirect table: FNV-1a with a 32-bit mask.
const HASH_MULTIPLIER: u32 = 0x0100_0193;
const HASH_SEED: u32 = HASH_MULTIPLIER;
const HASH_MASK: u32 = 0x7FFF_FFFF;

// Attribute kinds from `ImageLocation`.
const ATTRIBUTE_MODULE: usize = 1;
const ATTRIBUTE_PARENT: usize = 2;
const ATTRIBUTE_BASE: usize = 3;
const ATTRIBUTE_EXTENSION: usize = 4;
const ATTRIBUTE_OFFSET: usize = 5;
const ATTRIBUTE_COMPRESSED: usize = 6;
const ATTRIBUTE_UNCOMPRESSED: usize = 7;
const ATTRIBUTE_COUNT: usize = 8;

/// Why an image could not be read.
#[derive(Debug)]
pub enum ImageError {
    /// The first word was not [`MAGIC`].
    NotAnImage {
        magic: u32,
    },
    /// A version this reader does not implement.
    UnsupportedVersion {
        major: u16,
        minor: u16,
    },
    /// The file ended inside the header, the index, or a resource.
    Truncated {
        offset: usize,
        needed: usize,
        len: usize,
    },
    /// `resource_count` and `table_length` cannot describe a hash table.
    BadTable {
        resource_count: u32,
        table_length: u32,
    },
    /// A location record named an attribute kind outside the eight defined ones.
    BadAttribute {
        kind: u8,
    },
    /// A string in the string table ran past its end, or was not modified UTF-8.
    BadString {
        offset: usize,
    },
    /// A resource stored with a compression this reader does not implement. The reference
    /// images are uncompressed; a `jlink --compress` image would land here.
    Compressed {
        name: String,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnImage { magic } => {
                write!(f, "not an image: magic 0x{magic:08X}")
            }
            Self::UnsupportedVersion { major, minor } => {
                write!(f, "unsupported image version {major}.{minor}")
            }
            Self::Truncated {
                offset,
                needed,
                len,
            } => write!(
                f,
                "image truncated: wanted {needed} byte(s) at {offset}, file is {len}"
            ),
            Self::BadTable {
                resource_count,
                table_length,
            } => write!(
                f,
                "bad image index: {resource_count} resource(s) in a table of {table_length}"
            ),
            Self::BadAttribute { kind } => write!(f, "unknown location attribute {kind}"),
            Self::BadString { offset } => write!(f, "bad string at offset {offset}"),
            Self::Compressed { name } => {
                write!(f, "compressed image resource is not supported: {name}")
            }
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The seven header words, named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub flags: u32,
    pub resource_count: u32,
    pub table_length: u32,
    pub locations_size: u32,
    pub strings_size: u32,
}

impl Header {
    /// Where the index ends and the resource bytes begin.
    pub fn index_size(&self) -> usize {
        HEADER_SIZE
            + 4 * self.table_length as usize
            + 4 * self.table_length as usize
            + self.locations_size as usize
            + self.strings_size as usize
    }
}

/// One resource name from the index.
///
/// `name` is the full path, `/java.base/java/lang/Object.class`; `module` is the same entry
/// without the `/module/` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub module: String,
    pub name: String,
}

/// A read-only view of one `lib/modules` file.
#[derive(Debug)]
pub struct Image {
    bytes: Vec<u8>,
    header: Header,
}

impl Image {
    /// Opens a JDK home's `lib/modules`.
    pub fn open_jdk(home: impl AsRef<Path>) -> Result<Self, ImageError> {
        Self::open(home.as_ref().join("lib/modules"))
    }

    /// Opens an image file by path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ImageError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| ImageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_bytes(bytes)
    }

    /// Parses an image already in memory. The bytes are kept, not copied.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ImageError> {
        let len = bytes.len();
        if len < HEADER_SIZE {
            return Err(ImageError::Truncated {
                offset: 0,
                needed: HEADER_SIZE,
                len,
            });
        }
        let word = |index: usize| {
            let start = index * 4;
            u32::from_ne_bytes([
                bytes[start],
                bytes[start + 1],
                bytes[start + 2],
                bytes[start + 3],
            ])
        };
        let magic = word(0);
        if magic != MAGIC {
            return Err(ImageError::NotAnImage { magic });
        }
        let version = word(1);
        let major = (version >> 16) as u16;
        let minor = (version & 0xFFFF) as u16;
        if major != MAJOR_VERSION || minor != MINOR_VERSION {
            return Err(ImageError::UnsupportedVersion { major, minor });
        }
        let header = Header {
            magic,
            major_version: major,
            minor_version: minor,
            flags: word(2),
            resource_count: word(3),
            table_length: word(4),
            locations_size: word(5),
            strings_size: word(6),
        };
        if header.table_length == 0 || header.resource_count > header.table_length {
            return Err(ImageError::BadTable {
                resource_count: header.resource_count,
                table_length: header.table_length,
            });
        }
        let index_size = header.index_size();
        if index_size > len {
            return Err(ImageError::Truncated {
                offset: HEADER_SIZE,
                needed: index_size - HEADER_SIZE,
                len,
            });
        }
        Ok(Self { bytes, header })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The number of resources the index actually holds.
    ///
    /// This is what the table length counts down to: the slots that are not empty.
    pub fn resource_count(&self) -> usize {
        (0..self.header.table_length as usize)
            .filter(|index| self.offset_at(*index) != 0)
            .count()
    }

    /// Every entry in the index, in table order (which is hash order, not name order).
    ///
    /// An `Err` stops the iteration for that entry; a corrupt index reports itself instead of
    /// silently skipping.
    pub fn entries(&self) -> impl Iterator<Item = Result<Entry, ImageError>> + '_ {
        (0..self.header.table_length as usize)
            .filter(|index| self.offset_at(*index) != 0)
            .map(|index| self.entry_at(self.offset_at(index)))
    }

    /// Every entry of one module, sorted by name.
    pub fn module_entries(&self, module: &str) -> Result<Vec<Entry>, ImageError> {
        let mut entries: Vec<Entry> = self
            .entries()
            .filter_map(|entry| match entry {
                Ok(entry) if entry.module == module => Some(Ok(entry)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<_, _>>()?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    /// The bytes of a resource named by its full path, `/module/path/Name.class`.
    ///
    /// The lookup verifies the name it found, so a hash collision reports `None` rather than
    /// the wrong class.
    pub fn resource(&self, full_name: &str) -> Result<Option<&[u8]>, ImageError> {
        let Some(index) = self.location_index(full_name) else {
            return Ok(None);
        };
        let location = self.offset_at(index);
        if location == 0 {
            return Ok(None);
        }
        let attributes = self.attributes_at(location)?;
        if self.full_name(&attributes)? != full_name {
            return Ok(None);
        }
        let offset = self.header.index_size() + attributes[ATTRIBUTE_OFFSET] as usize;
        let size = attributes[ATTRIBUTE_UNCOMPRESSED] as usize;
        if attributes[ATTRIBUTE_COMPRESSED] != 0 {
            return Err(ImageError::Compressed {
                name: full_name.to_owned(),
            });
        }
        let end = offset.checked_add(size).ok_or(ImageError::Truncated {
            offset,
            needed: size,
            len: self.bytes.len(),
        })?;
        if end > self.bytes.len() {
            return Err(ImageError::Truncated {
                offset,
                needed: size,
                len: self.bytes.len(),
            });
        }
        Ok(Some(&self.bytes[offset..end]))
    }

    /// The class file for a binary name (`java/lang/Object`) from `java.base`.
    ///
    /// The module split is M2's problem; until then every boot class is in `java.base`,
    /// which is where the JDK puts all of them.
    pub fn class(&self, binary_name: &str) -> Result<Option<&[u8]>, ImageError> {
        self.resource(&format!("/java.base/{binary_name}.class"))
    }

    fn redirect_at(&self, index: usize) -> i32 {
        let start = HEADER_SIZE + index * 4;
        i32::from_ne_bytes([
            self.bytes[start],
            self.bytes[start + 1],
            self.bytes[start + 2],
            self.bytes[start + 3],
        ])
    }

    fn offset_at(&self, index: usize) -> u32 {
        let start = HEADER_SIZE + 4 * self.header.table_length as usize + index * 4;
        u32::from_ne_bytes([
            self.bytes[start],
            self.bytes[start + 1],
            self.bytes[start + 2],
            self.bytes[start + 3],
        ])
    }

    fn locations_offset(&self) -> usize {
        HEADER_SIZE + 8 * self.header.table_length as usize
    }

    fn strings_offset(&self) -> usize {
        self.locations_offset() + self.header.locations_size as usize
    }

    /// The table slot a name hashes to, then its redirect value: a direct index when
    /// negative, a rehash seed when positive, empty when zero.
    fn location_index(&self, name: &str) -> Option<usize> {
        let count = self.header.table_length;
        let slot = (hash(name, HASH_SEED) % count) as usize;
        let redirect = self.redirect_at(slot);
        if redirect < 0 {
            // The slot holds the index directly, as its two's complement.
            Some((-redirect - 1) as usize)
        } else if redirect > 0 {
            // The slot is shared: the value is the seed of the second, final hash.
            Some((hash(name, redirect as u32) % count) as usize)
        } else {
            None
        }
    }

    fn entry_at(&self, location: u32) -> Result<Entry, ImageError> {
        let attributes = self.attributes_at(location)?;
        // `//modules` and `//packages` are index bookkeeping, not resources: they carry no
        // module, so their `module` is empty rather than the first string in the table.
        let module = if attributes[ATTRIBUTE_MODULE] != 0 {
            self.string(attributes[ATTRIBUTE_MODULE])?
        } else {
            String::new()
        };
        Ok(Entry {
            module,
            name: self.full_name(&attributes)?,
        })
    }

    /// Decodes one location record into its attribute values, indexed by kind.
    fn attributes_at(&self, location: u32) -> Result<[u32; ATTRIBUTE_COUNT], ImageError> {
        let mut attributes = [0u32; ATTRIBUTE_COUNT];
        let end = self.strings_offset();
        if location as usize >= self.header.locations_size as usize {
            return Err(ImageError::Truncated {
                offset: location as usize,
                needed: 1,
                len: self.header.locations_size as usize,
            });
        }
        let mut cursor = self.locations_offset() + location as usize;
        if cursor >= end {
            return Err(ImageError::Truncated {
                offset: cursor,
                needed: 1,
                len: self.bytes.len(),
            });
        }
        loop {
            if cursor >= end {
                return Err(ImageError::Truncated {
                    offset: cursor,
                    needed: 1,
                    len: self.bytes.len(),
                });
            }
            let byte = self.bytes[cursor];
            cursor += 1;
            if byte <= 0x7 {
                return Ok(attributes);
            }
            let kind = (byte >> 3) as usize;
            if kind >= ATTRIBUTE_COUNT {
                return Err(ImageError::BadAttribute { kind: byte >> 3 });
            }
            let length = (byte & 0x7) as usize + 1;
            let value = self.read_big_endian(cursor, length)?;
            cursor += length;
            attributes[kind] = value;
        }
    }

    /// Attribute values are stored big-endian, unlike the header words.
    fn read_big_endian(&self, offset: usize, length: usize) -> Result<u32, ImageError> {
        let end = offset.checked_add(length).ok_or(ImageError::Truncated {
            offset,
            needed: length,
            len: self.bytes.len(),
        })?;
        if end > self.strings_offset() || end > self.bytes.len() {
            return Err(ImageError::Truncated {
                offset,
                needed: length,
                len: self.bytes.len(),
            });
        }
        Ok(self.bytes[offset..end]
            .iter()
            .fold(0u32, |value, byte| (value << 8) | u32::from(*byte)))
    }

    /// Rebuilds a location's full name the way `ImageLocation.getFullName` does.
    fn full_name(&self, attributes: &[u32; ATTRIBUTE_COUNT]) -> Result<String, ImageError> {
        let mut name = String::new();
        if attributes[ATTRIBUTE_MODULE] != 0 {
            name.push('/');
            name.push_str(&self.string(attributes[ATTRIBUTE_MODULE])?);
            name.push('/');
        }
        if attributes[ATTRIBUTE_PARENT] != 0 {
            name.push_str(&self.string(attributes[ATTRIBUTE_PARENT])?);
            name.push('/');
        }
        name.push_str(&self.string(attributes[ATTRIBUTE_BASE])?);
        if attributes[ATTRIBUTE_EXTENSION] != 0 {
            name.push('.');
            name.push_str(&self.string(attributes[ATTRIBUTE_EXTENSION])?);
        }
        Ok(name)
    }

    /// Reads one NUL-terminated modified-UTF-8 string from the string table.
    fn string(&self, offset: u32) -> Result<String, ImageError> {
        let start = self.strings_offset() + offset as usize;
        let end = self.strings_offset() + self.header.strings_size as usize;
        if start >= end {
            return Err(ImageError::BadString {
                offset: offset as usize,
            });
        }
        decode_mutf8(&self.bytes[start..end], offset as usize)
    }
}

/// The `jimage` hash: FNV-1a over the bytes, masked to a non-negative value.
fn hash(text: &str, seed: u32) -> u32 {
    let mut value = seed;
    for byte in text.bytes() {
        value = value.wrapping_mul(HASH_MULTIPLIER) ^ u32::from(byte);
    }
    value & HASH_MASK
}

/// Decodes the modified UTF-8 the string table holds until its NUL terminator.
fn decode_mutf8(bytes: &[u8], base: usize) -> Result<String, ImageError> {
    let mut text = String::new();
    let mut index = 0;
    loop {
        let Some(&byte) = bytes.get(index) else {
            return Err(ImageError::BadString {
                offset: base + index,
            });
        };
        match byte {
            0 => return Ok(text),
            0x01..=0x7F => {
                text.push(char::from(byte));
                index += 1;
            }
            _ if byte & 0xE0 == 0xC0 => {
                let continuation = *bytes.get(index + 1).ok_or(ImageError::BadString {
                    offset: base + index,
                })?;
                if continuation & 0xC0 != 0x80 {
                    return Err(ImageError::BadString {
                        offset: base + index,
                    });
                }
                let code = (u32::from(byte & 0x1F) << 6) | u32::from(continuation & 0x3F);
                text.push(char::from_u32(code).ok_or(ImageError::BadString {
                    offset: base + index,
                })?);
                index += 2;
            }
            _ if byte & 0xF0 == 0xE0 => {
                let second = *bytes.get(index + 1).ok_or(ImageError::BadString {
                    offset: base + index,
                })?;
                let third = *bytes.get(index + 2).ok_or(ImageError::BadString {
                    offset: base + index,
                })?;
                if second & 0xC0 != 0x80 || third & 0xC0 != 0x80 {
                    return Err(ImageError::BadString {
                        offset: base + index,
                    });
                }
                let code = (u32::from(byte & 0x0F) << 12)
                    | (u32::from(second & 0x3F) << 6)
                    | u32::from(third & 0x3F);
                text.push(char::from_u32(code).ok_or(ImageError::BadString {
                    offset: base + index,
                })?);
                index += 3;
            }
            _ => {
                return Err(ImageError::BadString {
                    offset: base + index,
                });
            }
        }
    }
}
