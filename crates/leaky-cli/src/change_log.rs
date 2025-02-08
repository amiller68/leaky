use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

use leaky_common::prelude::*;

use serde::{Deserialize, Serialize};

// TODO: this is an akward way to do this, i could probably
// constructs diffs better

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum StagedType {
    Added,
    Modified,
    Removed,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ChangeType {
    // Covers the base state of the files (unchanged since the last call to `push`)
    Base,
    // Covers completely new files and whether they've been modified
    //  Since the last call to `add`
    Added { modified: bool },
    // Covers files that have been modified since the last call to `push`
    //  that still exist, but have been modified
    Modified { processed: bool },
    // Covers files that have been removed since the last call to `push`
    Removed,
}

impl std::fmt::Display for ChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Base => "\x1b[0;32mBase\x1b[0m",
            Self::Added { .. } => "\x1b[0;32mAdded\x1b[0m",
            Self::Modified { .. } => "\x1b[0;33mModified\x1b[0m",
            Self::Removed => "\x1b[0;31mRemoved\x1b[0m",
        };
        write!(f, "{}", s)
    }
}

/// Tracks what files are in the local clone and their hashes
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ChangeLog(BTreeMap<PathBuf, (Cid, ChangeType)>);

impl Deref for ChangeLog {
    type Target = BTreeMap<PathBuf, (Cid, ChangeType)>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ChangeLog {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ChangeLog {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
}
