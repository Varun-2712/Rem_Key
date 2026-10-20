use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

/// Per-user directory for the agent's identity and trust store, e.g.
/// `~/Library/Application Support/PhoneInputRelay` or `%APPDATA%\PhoneInputRelay`.
pub fn default_data_dir() -> Result<PathBuf> {
    let base = dirs::data_dir().context("no per-user data directory on this platform")?;
    Ok(base.join("PhoneInputRelay"))
}

/// Writes `data` so that readers see either the old or the new file, never a partial
/// one, and (on Unix) so that only the current user can read it.
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let file_name = path.file_name().context("path has no file name")?.to_string_lossy();
    let tmp = dir.join(format!(".{}.tmp-{}", file_name, std::process::id()));
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_atomically_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("secret.json");
        write_private(&path, b"one").unwrap();
        write_private(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        // No temp files left behind.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn only_owner_can_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        write_private(&path, b"x").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
