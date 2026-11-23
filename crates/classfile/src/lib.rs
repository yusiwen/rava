//! Java class-file parsing.
//!
//! The parser keeps unknown attributes as raw bytes and retains the metadata the
//! runtime needs later (annotations, signatures, records, sealed permits, nestmates
//! and debug information). It depends on no other rava crate.

#![forbid(unsafe_code)]
