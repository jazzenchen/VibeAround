//! Shared storage utilities.
//!
//! Business modules own their event schemas. This module only provides
//! low-level persistence helpers that can be reused by workspace threads,
//! workspace events and future append-only indexes.

pub mod jsonl;
