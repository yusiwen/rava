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

/// A classpath searched in order: a list of directories and jars.
///
/// Each path is opened when the source is built. A directory is read with
/// [`DirectorySource`] and a file with [`JarSource`]; the first entry that supplies a
/// class wins, which is Java's classpath order. A class no entry supplies is `None`, so
/// the loader reports [`VmError::ClassNotFound`] exactly as it does for a single source.
#[derive(Debug, Default)]
pub struct ClassPathSource {
    entries: Vec<ClassPathEntry>,
}

#[derive(Debug)]
enum ClassPathEntry {
    Directory(DirectorySource),
    Jar(JarSource),
}

impl ClassPathSource {
    /// An empty classpath. Every lookup misses.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Open `paths` in order, treating a directory as a tree and anything else as a jar.
    ///
    /// A path that does not exist fails while opening its jar, so a mistyped entry is
    /// reported instead of silently changing which classes are reachable.
    pub fn from_paths<I, P>(paths: I) -> Result<Self, VmError>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        let mut source = Self::empty();
        for path in paths {
            source.add_path(path)?;
        }
        Ok(source)
    }

    /// Append one directory or jar to the search order.
    pub fn add_path(&mut self, path: impl Into<PathBuf>) -> Result<(), VmError> {
        let path = path.into();
        let entry = if path.is_dir() {
            ClassPathEntry::Directory(DirectorySource::new(path))
        } else {
            ClassPathEntry::Jar(JarSource::open(path)?)
        };
        self.entries.push(entry);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl ClassSource for ClassPathSource {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        for entry in &self.entries {
            let bytes = match entry {
                ClassPathEntry::Directory(source) => source.read(binary_name)?,
                ClassPathEntry::Jar(source) => source.read(binary_name)?,
            };
            if bytes.is_some() {
                return Ok(bytes);
            }
        }
        Ok(None)
    }
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

    /// Every file entry in the jar, sorted lexicographically.
    ///
    /// Directory entries are not returned: the central-directory parser drops names that
    /// end in `/`. The order is stable regardless of the hash map's iteration order.
    pub fn entry_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.entries.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// The `.class` entries in the jar, as jar paths (for example `java/lang/Object.class`),
    /// sorted lexicographically. These are entry names, not binary names.
    pub fn class_entry_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .entries
            .keys()
            .filter(|name| name.ends_with(".class"))
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    /// Reads one entry by its exact jar path. `Ok(None)` means the jar has no such entry.
    ///
    /// [`ClassSource::read`] is the binary-name view of this method; scanning tools that
    /// already hold entry names use this one directly.
    pub fn read_entry(&self, name: &str) -> Result<Option<Vec<u8>>, VmError> {
        self.bytes_for(name)
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
        self.read_entry(&Self::relative(binary_name))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rava-classpath-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn classpath_returns_the_first_entry_that_supplies_a_class() {
        let first = scratch("order-first");
        let second = scratch("order-second");
        std::fs::write(first.join("Hello.class"), b"first").unwrap();
        std::fs::write(second.join("Hello.class"), b"second").unwrap();

        let source = ClassPathSource::from_paths([first, second]).unwrap();
        assert_eq!(
            source.read("Hello").unwrap().as_deref(),
            Some(&b"first"[..])
        );
    }

    #[test]
    fn classpath_searches_directories_and_jars_in_order() {
        let empty = scratch("jar-empty");
        let jar = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.jar");

        let source = ClassPathSource::from_paths([empty, jar]).unwrap();
        let bytes = source
            .read("Hello")
            .unwrap()
            .expect("the jar supplies Hello");
        assert_eq!(&bytes[..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    }

    #[test]
    fn classpath_misses_are_none_and_an_empty_path_has_no_entries() {
        let empty = ClassPathSource::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.read("Hello").unwrap(), None);

        let source = ClassPathSource::from_paths([scratch("missing")]).unwrap();
        assert_eq!(source.len(), 1);
        assert_eq!(source.read("Hello").unwrap(), None);
    }
}
