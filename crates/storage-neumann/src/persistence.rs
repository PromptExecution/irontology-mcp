/// Thin persistence abstraction over a key-value store.
///
/// Each logical namespace ("files", "symbols", "facts", etc.) is addressed by
/// a `tree` name string.  Keys and values are raw byte slices so the caller
/// owns serialisation.
///
/// The trait is object-safe and `Send + Sync` so it can be wrapped in
/// `Arc<dyn PersistenceBackend>` and shared across async tasks.
pub trait PersistenceBackend: Send + Sync {
    /// Write `value` under `key` in the named `tree`.
    fn upsert(&self, tree: &str, key: &[u8], value: &[u8]) -> anyhow::Result<()>;

    /// Return all (key, value) pairs stored in the named `tree`.
    fn scan(&self, tree: &str) -> anyhow::Result<Vec<(Vec<u8>, Vec<u8>)>>;
}

// ---------------------------------------------------------------------------
// Concrete sled 0.34 backend
// ---------------------------------------------------------------------------

/// A [`PersistenceBackend`] backed by sled 0.34.
pub struct SledBackend {
    db: sled::Db,
}

impl SledBackend {
    /// Open (or create) the database at `path`.
    pub fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        let db = sled::open(path)
            .map_err(|e| anyhow::anyhow!("SledBackend: failed to open sled at {}: {e}", path.display()))?;
        Ok(Self { db })
    }
}

impl PersistenceBackend for SledBackend {
    fn upsert(&self, tree: &str, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        let t = self
            .db
            .open_tree(tree)
            .map_err(|e| anyhow::anyhow!("SledBackend: cannot open tree '{tree}': {e}"))?;
        t.insert(key, value)
            .map_err(|e| anyhow::anyhow!("SledBackend: insert failed in tree '{tree}': {e}"))?;
        Ok(())
    }

    fn scan(&self, tree: &str) -> anyhow::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let t = self
            .db
            .open_tree(tree)
            .map_err(|e| anyhow::anyhow!("SledBackend: cannot open tree '{tree}': {e}"))?;
        let mut pairs = Vec::new();
        for item in t.iter() {
            let (k, v) = item
                .map_err(|e| anyhow::anyhow!("SledBackend: scan error in tree '{tree}': {e}"))?;
            pairs.push((k.to_vec(), v.to_vec()));
        }
        Ok(pairs)
    }
}
