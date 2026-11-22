//! Which files are being decoded right now, kept in a small file in the app data folder.
//! If the app stops without a clean exit (a crash the panic guard can't catch, like
//! running out of memory), the next launch finds those files listed and gives each a
//! strike (`Catalog::strike`). A file that's there twice is a problem file.
//!
//! This writes only to the app data folder, never to the music folder.

use super::store;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const FILE: &str = "reading.json";

pub struct Journal {
    /// Files being read, with how many readers each (tags and analysis can overlap).
    open: Mutex<BTreeMap<String, u32>>,
}

/// Removes its file from the journal when dropped (the read finished, or failed).
pub struct Reading {
    journal: Arc<Journal>,
    id: String,
}

impl Drop for Reading {
    fn drop(&mut self) {
        let mut open = lock(&self.journal.open);
        if let Some(n) = open.get_mut(&self.id) {
            *n -= 1;
            if *n == 0 {
                open.remove(&self.id);
            }
        }
        save(&open);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn save(open: &BTreeMap<String, u32>) {
    let ids: Vec<&String> = open.keys().collect();
    let _ = store::save(FILE, &ids);
}

impl Journal {
    /// Start a journal, returning the files the last run was reading when it stopped
    /// (empty after a clean exit).
    pub fn open() -> (Arc<Journal>, Vec<String>) {
        let left: Vec<String> = store::load(FILE);
        save(&BTreeMap::new());
        let j = Journal {
            open: Mutex::new(BTreeMap::new()),
        };
        (Arc::new(j), left)
    }

    /// Note that `id` is being read until the returned guard is dropped.
    pub fn reading(self: &Arc<Self>, id: &str) -> Reading {
        let mut open = lock(&self.open);
        *open.entry(id.to_string()).or_default() += 1;
        save(&open);
        Reading {
            journal: self.clone(),
            id: id.to_string(),
        }
    }

    /// A clean exit: nothing being read counts against its file.
    pub fn close(&self) {
        lock(&self.open).clear();
        save(&BTreeMap::new());
    }
}
