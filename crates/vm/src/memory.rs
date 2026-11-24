//! VM-owned memory regions.
//!
//! `MemoryRegion` is the M0 seam for the `MemoryRegion`/`DirectBuffer` abstraction
//! (ADR-0012). The backing store is a boxed host slice and the address is a fake,
//! VM-assigned token — never a host pointer and never dereferenced. S2-raw replaces
//! the backing store with a real arena; callers keep the same address and bounds API.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// The first fake address handed out. It is aligned and never zero.
pub const BASE_ADDRESS: u64 = 0x1000;

const ALIGN: u64 = 8;

static NEXT_ADDRESS: AtomicU64 = AtomicU64::new(BASE_ADDRESS);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    OutOfBounds {
        offset: usize,
        len: usize,
        region_len: usize,
    },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds {
                offset,
                len,
                region_len,
            } => write!(
                f,
                "region range {offset}..{} exceeds the region length {region_len}",
                offset.saturating_add(*len)
            ),
        }
    }
}

impl std::error::Error for MemoryError {}

/// A contiguous, VM-owned byte region with a fake address.
#[derive(Debug, PartialEq, Eq)]
pub struct MemoryRegion {
    address: u64,
    bytes: Box<[u8]>,
}

impl MemoryRegion {
    /// Allocate a zeroed region with a fresh fake address. The address is unique for
    /// the lifetime of the process and aligned; it is an opaque token, not a pointer.
    pub fn allocate(len: usize) -> Self {
        let size = aligned_size(len);
        let address = NEXT_ADDRESS.fetch_add(size, Ordering::Relaxed);
        Self {
            address,
            bytes: vec![0; len].into_boxed_slice(),
        }
    }

    /// Wrap existing bytes at an explicit fake address. The seam for a later arena
    /// that hands out addresses itself; M0 only calls this from tests.
    pub fn at(address: u64, bytes: impl Into<Box<[u8]>>) -> Self {
        Self {
            address,
            bytes: bytes.into(),
        }
    }

    /// The fake address of the first byte. Opaque; never cast to a pointer.
    pub fn address(&self) -> u64 {
        self.address
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    pub fn read(&self, offset: usize, len: usize) -> Result<&[u8], MemoryError> {
        let end = end_of(offset, len, self.bytes.len())?;
        Ok(&self.bytes[offset..end])
    }

    pub fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), MemoryError> {
        let end = end_of(offset, data.len(), self.bytes.len())?;
        self.bytes[offset..end].copy_from_slice(data);
        Ok(())
    }

    pub fn fill(&mut self, offset: usize, len: usize, value: u8) -> Result<(), MemoryError> {
        let end = end_of(offset, len, self.bytes.len())?;
        self.bytes[offset..end].fill(value);
        Ok(())
    }
}

fn end_of(offset: usize, len: usize, region_len: usize) -> Result<usize, MemoryError> {
    let end = offset.checked_add(len);
    match end {
        Some(end) if end <= region_len => Ok(end),
        _ => Err(MemoryError::OutOfBounds {
            offset,
            len,
            region_len,
        }),
    }
}

fn aligned_size(len: usize) -> u64 {
    let len = len as u64;
    len.max(ALIGN).div_ceil(ALIGN) * ALIGN
}
