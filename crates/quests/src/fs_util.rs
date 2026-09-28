//! Small filesystem helper shared by [`crate::progress`] and [`crate::captured_state`]: write JSON
//! to a uniquely-named temp file in the same directory, fsync it, then rename it over the target so
//! a reader (or a crash) never observes a half-written file.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Error;

/// A monotonically increasing suffix for temp file names, so concurrent writers on different
/// threads of the same process never collide (Python used the OS process id plus thread id).
static NEXT_TEMP_SUFFIX: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic_write_json(path: &Path, payload: &serde_json::Value) -> Result<(), Error> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("data.json");
    let unique = NEXT_TEMP_SUFFIX.fetch_add(1, Ordering::Relaxed);
    let temp_path = dir.join(format!(".{file_name}.{}.{unique}.tmp", std::process::id()));

    let result = (|| -> Result<(), Error> {
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(&serde_json::to_vec_pretty(payload)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}
