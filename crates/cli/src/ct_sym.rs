//! A reader for the JDK's `lib/ct.sym` signature archive.
//!
//! `ct.sym` is a ZIP whose entry names encode the Java releases a signature applies to:
//! `GH/java.base/java/lang/Object.sig` is the `java.lang.Object` signature shared by
//! releases 16 (`G`) and 17 (`H`), and `I/...` is the one for release 18. The archive
//! holds class files with the host JDK's class-file version, no `Code` attributes and no
//! member implementations, which is exactly the signature reference the C1 gate needs.
//!
//! Only what this tool needs is implemented: the central directory, stored and deflated
//! entries, and a release-letter filter. ZIP64 and encryption are rejected with a message
//! instead of being half-supported.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const EOCD_MIN_LEN: usize = 22;
const CENTRAL_MIN_LEN: usize = 46;
const LOCAL_MIN_LEN: usize = 30;
const ZIP64_SENTINEL: u32 = 0xFFFF_FFFF;

/// A malformed or unreadable `ct.sym`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtSymError(String);

impl CtSymError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CtSymError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One selected `.sig` entry in the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    name: String,
    flags: u16,
    method: u16,
    crc: u32,
    compressed_size: usize,
    uncompressed_size: usize,
    local_offset: usize,
}

/// The release-filtered index over one `ct.sym` archive.
pub struct CtSym {
    path: PathBuf,
    release: char,
    archive: Vec<u8>,
    entries: BTreeMap<String, Vec<Entry>>,
}

impl CtSym {
    /// Opens `path` and indexes the `.sig` entries that apply to `release`.
    ///
    /// `release` is the one-character release letter `ct.sym` uses: `8`, `9`, `A` for 10,
    /// and so on, so Java 17 is `H`.
    pub fn open(path: &Path, release: char) -> Result<Self, CtSymError> {
        let archive = fs::read(path).map_err(|error| {
            CtSymError::new(format!(
                "cannot read ct.sym {}: {error}; pass --ct-sym <path> or set RAVA_CT_SYM",
                path.display()
            ))
        })?;
        let mut entries: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
        for entry in read_central_directory(&archive, path)? {
            let Some((prefix, rest)) = entry.name.split_once('/') else {
                continue;
            };
            if !prefix.chars().any(|character| character == release) {
                continue;
            }
            let Some((_, signature_path)) = rest.split_once('/') else {
                continue;
            };
            let Some(owner) = signature_path.strip_suffix(".sig") else {
                continue;
            };
            if owner == "module-info" {
                continue;
            }
            entries.entry(owner.to_owned()).or_default().push(entry);
        }
        Ok(Self {
            path: path.to_path_buf(),
            release,
            archive,
            entries,
        })
    }

    /// The archive path, for reports and error messages.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when the archive has no signature at all for this release, which usually
    /// means the archive belongs to an older JDK.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when `owner` has at least one signature for this release.
    ///
    /// An owner with several release signatures is ambiguous: [`Self::read`] rejects it
    /// rather than choosing one.
    pub fn contains(&self, owner: &str) -> bool {
        self.entries.contains_key(owner)
    }

    /// Reads the release signature of `owner`.
    ///
    /// An absent owner is an error that names it; an owner with more than one signature in
    /// the release is an ambiguity, not a choice this tool is allowed to make silently.
    pub fn read(&self, owner: &str) -> Result<Vec<u8>, CtSymError> {
        let entries = self.entries.get(owner).ok_or_else(|| {
            CtSymError::new(format!(
                "no release-{} ct.sym signature for class {owner}",
                self.release
            ))
        })?;
        if entries.len() != 1 {
            let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
            return Err(CtSymError::new(format!(
                "class {owner} has {} release-{} ct.sym signatures ({}); the reference is ambiguous",
                entries.len(),
                self.release,
                names.join(", ")
            )));
        }
        self.read_entry(&entries[0])
    }

    fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>, CtSymError> {
        if entry.flags & 0x0001 != 0 {
            return Err(CtSymError::new(format!(
                "ct.sym entry {} is encrypted, which is not supported",
                entry.name
            )));
        }
        let header = entry.local_offset;
        let header_end = header
            .checked_add(LOCAL_MIN_LEN)
            .ok_or_else(|| CtSymError::new("ct.sym local header offset overflow"))?;
        if header_end > self.archive.len()
            || u32_at(&self.archive, header).ok() != Some(LOCAL_SIGNATURE)
        {
            return Err(CtSymError::new(format!(
                "ct.sym entry {} has no valid local header",
                entry.name
            )));
        }
        let name_len = u16_at(&self.archive, header + 26)? as usize;
        let extra_len = u16_at(&self.archive, header + 28)? as usize;
        let data_start = header_end
            .checked_add(name_len)
            .and_then(|value| value.checked_add(extra_len))
            .ok_or_else(|| CtSymError::new("ct.sym entry offset overflow"))?;
        let data_end = data_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| CtSymError::new("ct.sym entry size overflow"))?;
        let compressed = self
            .archive
            .get(data_start..data_end)
            .ok_or_else(|| CtSymError::new(format!("ct.sym entry {} is truncated", entry.name)))?;

        let bytes = match entry.method {
            0 => compressed.to_vec(),
            8 => {
                let mut decoder = flate2::read::DeflateDecoder::new(compressed);
                let mut output = Vec::with_capacity(entry.uncompressed_size);
                decoder.read_to_end(&mut output).map_err(|error| {
                    CtSymError::new(format!(
                        "cannot inflate ct.sym entry {}: {error}",
                        entry.name
                    ))
                })?;
                output
            }
            other => {
                return Err(CtSymError::new(format!(
                    "ct.sym entry {} uses unsupported compression method {other}",
                    entry.name
                )));
            }
        };

        if bytes.len() != entry.uncompressed_size {
            return Err(CtSymError::new(format!(
                "ct.sym entry {} inflates to {} bytes but declares {}",
                entry.name,
                bytes.len(),
                entry.uncompressed_size
            )));
        }
        if crc32(&bytes) != entry.crc {
            return Err(CtSymError::new(format!(
                "ct.sym entry {} fails its CRC check",
                entry.name
            )));
        }
        Ok(bytes)
    }
}

/// The one-character release letter `ct.sym` uses for `java_major`.
///
/// Java 8 and 9 are the digits; Java 10 onward are `A`, `B`, … so 17 is `H`.
pub fn release_letter(java_major: u32) -> Result<char, CtSymError> {
    match java_major {
        1..=9 => char::from_digit(java_major, 10)
            .ok_or_else(|| CtSymError::new(format!("invalid Java major version {java_major}"))),
        10..=35 => char::from_u32(u32::from(b'A') + (java_major - 10))
            .ok_or_else(|| CtSymError::new(format!("invalid Java major version {java_major}"))),
        other => Err(CtSymError::new(format!(
            "Java major version {other} is outside the ct.sym release range"
        ))),
    }
}

fn read_central_directory(archive: &[u8], path: &Path) -> Result<Vec<Entry>, CtSymError> {
    let eocd = find_eocd(archive).ok_or_else(|| {
        CtSymError::new(format!(
            "{} is not a ZIP archive: no end-of-central-directory record",
            path.display()
        ))
    })?;
    let entries_on_disk = u16_at(archive, eocd + 8)?;
    let total_entries = u16_at(archive, eocd + 10)?;
    let directory_size = u32_at(archive, eocd + 12)? as usize;
    let directory_offset = u32_at(archive, eocd + 16)? as usize;
    if entries_on_disk != total_entries {
        return Err(CtSymError::new(format!(
            "{} spans multiple ZIP disks, which is not supported",
            path.display()
        )));
    }
    if directory_offset == ZIP64_SENTINEL as usize || directory_size == ZIP64_SENTINEL as usize {
        return Err(CtSymError::new(format!(
            "{} is a ZIP64 archive, which is not supported",
            path.display()
        )));
    }
    let directory_end = directory_offset
        .checked_add(directory_size)
        .ok_or_else(|| CtSymError::new("ct.sym central directory offset overflow"))?;
    if directory_end > archive.len() {
        return Err(CtSymError::new(format!(
            "{} has a central directory past the end of the file",
            path.display()
        )));
    }

    let mut entries = Vec::with_capacity(usize::from(total_entries));
    let mut offset = directory_offset;
    for _ in 0..total_entries {
        let fixed_end = offset
            .checked_add(CENTRAL_MIN_LEN)
            .ok_or_else(|| CtSymError::new("ct.sym central directory offset overflow"))?;
        if fixed_end > directory_end || u32_at(archive, offset)? != CENTRAL_SIGNATURE {
            return Err(CtSymError::new(
                "ct.sym central directory has a malformed entry header",
            ));
        }
        let name_len = u16_at(archive, offset + 28)? as usize;
        let extra_len = u16_at(archive, offset + 30)? as usize;
        let comment_len = u16_at(archive, offset + 32)? as usize;
        let name_start = fixed_end;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| CtSymError::new("ct.sym entry name overflow"))?;
        let name = archive
            .get(name_start..name_end)
            .ok_or_else(|| CtSymError::new("ct.sym entry name is truncated"))?;
        let name = String::from_utf8_lossy(name).into_owned();

        let compressed_size = u32_at(archive, offset + 20)? as usize;
        let uncompressed_size = u32_at(archive, offset + 24)? as usize;
        let local_offset = u32_at(archive, offset + 42)? as usize;
        if compressed_size == ZIP64_SENTINEL as usize
            || uncompressed_size == ZIP64_SENTINEL as usize
            || local_offset == ZIP64_SENTINEL as usize
        {
            return Err(CtSymError::new(format!(
                "ct.sym entry {name} uses ZIP64 fields, which is not supported"
            )));
        }
        entries.push(Entry {
            name,
            flags: u16_at(archive, offset + 8)?,
            method: u16_at(archive, offset + 10)?,
            crc: u32_at(archive, offset + 16)?,
            compressed_size,
            uncompressed_size,
            local_offset,
        });

        offset = fixed_end
            .checked_add(name_len)
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(comment_len))
            .ok_or_else(|| CtSymError::new("ct.sym central directory offset overflow"))?;
    }
    Ok(entries)
}

fn find_eocd(archive: &[u8]) -> Option<usize> {
    if archive.len() < EOCD_MIN_LEN {
        return None;
    }
    let last = archive.len() - EOCD_MIN_LEN;
    let first = last.saturating_sub(0xFFFF);
    (first..=last).rev().find(|&offset| {
        u32::from_le_bytes([
            archive[offset],
            archive[offset + 1],
            archive[offset + 2],
            archive[offset + 3],
        ]) == EOCD_SIGNATURE
            && offset + EOCD_MIN_LEN + usize::from(u16_at_lossy(archive, offset + 20))
                == archive.len()
    })
}

fn u16_at(archive: &[u8], offset: usize) -> Result<u16, CtSymError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| CtSymError::new("ct.sym offset overflow"))?;
    let bytes = archive
        .get(offset..end)
        .ok_or_else(|| CtSymError::new("ct.sym field is truncated"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u16_at_lossy(archive: &[u8], offset: usize) -> u16 {
    u16_at(archive, offset).unwrap_or(0)
}

fn u32_at(archive: &[u8], offset: usize) -> Result<u32, CtSymError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| CtSymError::new("ct.sym offset overflow"))?;
    let bytes = archive
        .get(offset..end)
        .ok_or_else(|| CtSymError::new("ct.sym field is truncated"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Builds a minimal ZIP in memory. `deflate` selects method 8 over method 0.
    fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in entries {
            let offset = out.len();
            let compressed = if *deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                std::io::Write::write_all(&mut encoder, data).expect("compress");
                encoder.finish().expect("finish compression")
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let crc = crc32(data);

            out.extend_from_slice(&LOCAL_SIGNATURE.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&compressed);

            central.extend_from_slice(&CENTRAL_SIGNATURE.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&(offset as u32).to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }

        let directory_offset = out.len();
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&(directory_offset as u32).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn archive(entries: &[(&str, &[u8], bool)]) -> (PathBuf, CtSym) {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rava-ct-sym-{}-{id}.zip", std::process::id()));
        fs::write(&path, zip(entries)).expect("write synthetic ct.sym");
        let ct_sym = CtSym::open(&path, 'H').expect("open synthetic ct.sym");
        (path, ct_sym)
    }

    #[test]
    fn maps_java_releases_to_ct_sym_letters() {
        assert_eq!(release_letter(8).unwrap(), '8');
        assert_eq!(release_letter(9).unwrap(), '9');
        assert_eq!(release_letter(10).unwrap(), 'A');
        assert_eq!(release_letter(17).unwrap(), 'H');
        assert_eq!(release_letter(25).unwrap(), 'P');
        assert!(release_letter(36).is_err());
    }

    #[test]
    fn selects_release_entries_and_reads_stored_and_deflated_data() {
        let (path, ct_sym) = archive(&[
            ("GH/java.base/demo/Stored.sig", b"stored", false),
            ("GH/java.base/demo/Deflated.sig", b"deflated", true),
            ("I/java.base/demo/Newer.sig", b"newer", false),
            ("GH/java.base/module-info.sig", b"module", false),
        ]);
        assert!(ct_sym.contains("demo/Stored"));
        assert!(ct_sym.contains("demo/Deflated"));
        assert!(!ct_sym.contains("demo/Newer"));
        assert_eq!(ct_sym.read("demo/Stored").unwrap(), b"stored");
        assert_eq!(ct_sym.read("demo/Deflated").unwrap(), b"deflated");
        let error = ct_sym.read("demo/Newer").expect_err("wrong release");
        assert!(error.to_string().contains("release-H"), "{error}");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn an_ambiguous_owner_is_rejected() {
        let (path, ct_sym) = archive(&[
            ("GH/java.base/demo/Dup.sig", b"a", false),
            ("GH/java.sql/demo/Dup.sig", b"b", false),
        ]);
        let error = ct_sym.read("demo/Dup").expect_err("ambiguous");
        assert!(error.to_string().contains("ambiguous"), "{error}");
        let _ = fs::remove_file(path);
    }
}
