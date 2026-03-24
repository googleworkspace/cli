// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! File-system utilities.

use std::io::{self, Write};
use std::path::Path;

/// Write `data` to `path` atomically.
///
/// This implementation uses `tempfile::NamedTempFile` to create a temporary
/// file with a random name, `O_EXCL` flags (preventing symlink attacks),
/// and secure 0600 permissions from the moment of creation.
///
/// # Errors
///
/// Returns an `io::Error` if the temporary file cannot be created/written or if the
/// final rename fails.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .map_err(|e| io::Error::new(e.error.kind(), e.error))?;

    Ok(())
}

/// Async variant of [`atomic_write`] for use with tokio.
///
/// This implementation uses `create_new(true)` (O_EXCL) and `mode(0o600)` to
/// prevent TOCTOU/symlink race conditions.
pub async fn atomic_write_async(path: &Path, data: &[u8]) -> io::Result<()> {
    use rand::Rng;
    use tokio::io::AsyncWriteExt;

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();

    let mut retries = 0;
    let mut file: tokio::fs::File;
    let mut tmp_path;

    loop {
        let suffix: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();
        let tmp_name = format!("{}.tmp.{}", file_name, suffix);
        tmp_path = parent.join(tmp_name);

        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true).create_new(true);

        #[cfg(unix)]
        {
            opts.mode(0o600);
        }

        match opts.open(&tmp_path).await {
            Ok(f) => {
                file = f;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && retries < 10 => {
                retries += 1;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    let write_result = async {
        file.write_all(data).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&tmp_path, path).await
    }
    .await;

    if write_result.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }

    write_result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn test_atomic_write_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        atomic_write(&path, b"data").unwrap();
        // Since we use random names, we just check that no .tmp files remain in the dir
        let files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|res| res.unwrap().file_name())
            .collect();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], "credentials.enc");
    }

    #[tokio::test]
    async fn test_atomic_write_async_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token_cache.json");
        atomic_write_async(&path, b"async hello").await.unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"async hello");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(&path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }
}
