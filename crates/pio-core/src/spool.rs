//! Immutable content-addressed bytes. References, not payloads, enter the journal.
use anyhow::{Result, ensure};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub struct Spool {
    root: PathBuf,
}
impl Spool {
    pub fn open(store: &Path) -> Result<Self> {
        let root = store.join("spool");
        if !root.exists() {
            std::fs::create_dir(&root)?;
        }
        ensure!(
            std::fs::symlink_metadata(&root)?.is_dir(),
            "invalid spool directory"
        );
        Ok(Self { root })
    }
    fn path(&self, digest: &str) -> Result<PathBuf> {
        let hex = digest.strip_prefix("sha256:").unwrap_or("");
        ensure!(
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid spool digest"
        );
        Ok(self.root.join(hex))
    }
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let digest = crate::digest(bytes);
        let path = self.path(&digest)?;
        if path.exists() {
            ensure!(self.read(&digest)? == bytes, "spool digest conflict");
            return Ok(digest);
        }
        let temp = self.root.join(format!(".pending-{}", uuid::Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)?;
        File::open(&self.root)?.sync_all()?;
        Ok(digest)
    }
    pub fn read(&self, digest: &str) -> Result<Vec<u8>> {
        let path = self.path(digest)?;
        ensure!(
            std::fs::symlink_metadata(&path)?.is_file(),
            "invalid spool object"
        );
        let bytes = std::fs::read(path)?;
        ensure!(crate::digest(&bytes) == digest, "spool digest mismatch");
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_bytes_are_verified_and_missing_or_corrupt_is_not_empty_output() {
        let root = tempfile::tempdir().unwrap();
        let spool = Spool::open(root.path()).unwrap();
        let digest = spool.put(b"observed output").unwrap();
        assert_eq!(spool.put(b"observed output").unwrap(), digest);
        assert_eq!(spool.read(&digest).unwrap(), b"observed output");
        std::fs::write(spool.path(&digest).unwrap(), b"tampered").unwrap();
        assert!(spool.read(&digest).is_err());
        assert!(spool.read("sha256:../escape").is_err());
    }
}
