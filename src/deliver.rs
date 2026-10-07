use crate::error::{HdrError, HdrResult};
use std::path::Path;

pub fn atomic_write(path: &Path, contents: &[u8]) -> HdrResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|source| HdrError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
    }
    let mut tmp = path.to_path_buf();
    tmp.set_file_name(format!(".{}.hdr_probe254.tmp",
        path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()));
    std::fs::write(&tmp, contents).map_err(|source| HdrError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| HdrError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}
