//! Sources of class bytes: a directory or a jar.
//!
//! A source is deliberately independent of the loader; the loader decides which
//! source to ask and records the defining loader.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::read::DeflateDecoder;

use crate::error::VmError;

/// A source of class bytes, addressed by binary name.
pub trait ClassSource: Send + Sync + std::fmt::Debug {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError>;
}

/// A directory tree containing `<binary/name>.class` files.
#[derive(Debug, Clone)]
pub struct DirectorySource {
    root: PathBuf,
}

impl DirectorySource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn relative(binary_name: &str) -> String {
        format!("{}.class", binary_name.replace('.', "/"))
    }
}

impl ClassSource for DirectorySource {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        let path = self.root.join(Self::relative(binary_name));
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(VmError::Io { path, source }),
        }
    }
}

#[derive(Debug)]
struct JarEntry {
    method: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    local_offset: usize,
}

/// A jar (zip) source. Entries are read from the central directory; stored and
/// DEFLATE entries are supported. Zip64 is rejected for now.
#[derive(Debug)]
pub struct JarSource {
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    entries: HashMap<String, JarEntry>,
}

impl JarSource {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, VmError> {
        let path = path.into();
        let bytes = std::fs::read(&path).map_err(|source| VmError::Io {
            path: path.clone(),
            source,
        })?;
        let entries = central_directory(&bytes)?;
        Ok(Self {
            path,
            bytes: Arc::new(bytes),
            entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn relative(binary_name: &str) -> String {
        format!("{}.class", binary_name.replace('.', "/"))
    }

    fn bytes_for(&self, name: &str) -> Result<Option<Vec<u8>>, VmError> {
        let Some(entry) = self.entries.get(name) else {
            return Ok(None);
        };
        let bytes = &self.bytes;
        let local = entry.local_offset;
        if local + 30 > bytes.len() || u32_at(bytes, local) != Some(0x0403_4b50) {
            return Err(VmError::Jar(format!("bad local header for {name}")));
        }
        let name_len = u16_at(bytes, local + 26).unwrap_or(0) as usize;
        let extra_len = u16_at(bytes, local + 28).unwrap_or(0) as usize;
        let data_start = local + 30 + name_len + extra_len;
        let data_end = data_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| VmError::Jar(format!("entry {name} overflows")))?;
        let data = bytes
            .get(data_start..data_end)
            .ok_or_else(|| VmError::Jar(format!("entry {name} points outside the jar")))?;

        match entry.method {
            0 => {
                if data.len() != entry.uncompressed_size {
                    return Err(VmError::Jar(format!(
                        "stored entry {name} has inconsistent sizes"
                    )));
                }
                Ok(Some(data.to_vec()))
            }
            8 => {
                let mut decoder = DeflateDecoder::new(data);
                let mut out = Vec::with_capacity(entry.uncompressed_size);
                decoder
                    .read_to_end(&mut out)
                    .map_err(|error| VmError::Jar(format!("cannot inflate {name}: {error}")))?;
                if out.len() != entry.uncompressed_size {
                    return Err(VmError::Jar(format!(
                        "deflated entry {name} has inconsistent sizes"
                    )));
                }
                Ok(Some(out))
            }
            method => Err(VmError::Jar(format!(
                "entry {name} uses unsupported compression method {method}"
            ))),
        }
    }
}

impl ClassSource for JarSource {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        self.bytes_for(&Self::relative(binary_name))
    }
}

fn central_directory(bytes: &[u8]) -> Result<HashMap<String, JarEntry>, VmError> {
    let eocd = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|&offset| u32_at(bytes, offset) == Some(0x0605_4b50))
        .ok_or_else(|| VmError::Jar("end-of-central-directory record not found".to_owned()))?;

    let entry_count = u16_at(bytes, eocd + 10).unwrap_or(0) as usize;
    let directory_offset = u32_at(bytes, eocd + 16).unwrap_or(0) as usize;
    let mut entries = HashMap::with_capacity(entry_count);
    let mut cursor = directory_offset;

    for _ in 0..entry_count {
        if u32_at(bytes, cursor) != Some(0x0201_4b50) {
            return Err(VmError::Jar("bad central-directory entry".to_owned()));
        }
        let method = u16_at(bytes, cursor + 10).unwrap_or(0);
        let compressed_size = u32_at(bytes, cursor + 20).unwrap_or(0) as usize;
        let uncompressed_size = u32_at(bytes, cursor + 24).unwrap_or(0) as usize;
        let name_len = u16_at(bytes, cursor + 28).unwrap_or(0) as usize;
        let extra_len = u16_at(bytes, cursor + 30).unwrap_or(0) as usize;
        let comment_len = u16_at(bytes, cursor + 32).unwrap_or(0) as usize;
        let local_offset = u32_at(bytes, cursor + 42).unwrap_or(0) as usize;
        let name_start = cursor + 46;
        let name_end = name_start + name_len;
        let name = bytes
            .get(name_start..name_end)
            .ok_or_else(|| VmError::Jar("entry name points outside the jar".to_owned()))?;
        let name = String::from_utf8(name.to_vec())
            .map_err(|_| VmError::Jar("entry name is not UTF-8".to_owned()))?;

        if compressed_size == u32::MAX as usize
            || uncompressed_size == u32::MAX as usize
            || local_offset == u32::MAX as usize
        {
            return Err(VmError::Jar(format!(
                "entry {name} uses Zip64, which is not supported yet"
            )));
        }
        if !name.ends_with('/') {
            entries.insert(
                name,
                JarEntry {
                    method,
                    compressed_size,
                    uncompressed_size,
                    local_offset,
                },
            );
        }
        cursor = name_end + extra_len + comment_len;
    }
    Ok(entries)
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}
