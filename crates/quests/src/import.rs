//! Taking over the quest progress DnDTools saved, once, when the app has none of its own yet.
//! DnDTools' file is only ever read: it is copied, never moved or changed.

use std::path::Path;

use crate::progress::PROGRESS_FILE_NAME;
use crate::Error;

/// Copies `<source_dir>/quests_progress.json` into `dest_dir` unless `dest_dir` already has a
/// progress file (or there is nothing to copy). A file that isn't valid JSON is not taken over.
/// Returns whether a file was copied.
pub fn import_progress_once(source_dir: &Path, dest_dir: &Path) -> Result<bool, Error> {
    let source = source_dir.join(PROGRESS_FILE_NAME);
    let dest = dest_dir.join(PROGRESS_FILE_NAME);
    if dest.exists() || !source.is_file() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(&source)?;
    serde_json::from_str::<serde_json::Value>(&text)?;
    std::fs::create_dir_all(dest_dir)?;
    // Written under another name first, so an interrupted copy is never taken for a finished one.
    let partial = dest_dir.join(format!("{PROGRESS_FILE_NAME}.partial"));
    std::fs::write(&partial, &text)?;
    std::fs::rename(&partial, &dest)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAVED: &str = r#"{"version": 2, "progress": {"objectives": {}, "items": {}}}"#;

    #[test]
    fn copies_once_and_leaves_the_original_alone() {
        let source = tempfile::tempdir().expect("temp dir");
        let dest = tempfile::tempdir().expect("temp dir");
        let dest_dir = dest.path().join("quests");
        std::fs::write(source.path().join(PROGRESS_FILE_NAME), SAVED).expect("write source");

        assert!(import_progress_once(source.path(), &dest_dir).expect("import"));
        assert_eq!(std::fs::read_to_string(dest_dir.join(PROGRESS_FILE_NAME)).expect("copied"), SAVED);
        assert_eq!(std::fs::read_to_string(source.path().join(PROGRESS_FILE_NAME)).expect("source"), SAVED);

        // Our own file is never replaced, even when DnDTools' changes later.
        std::fs::write(dest_dir.join(PROGRESS_FILE_NAME), "{}").expect("own progress");
        assert!(!import_progress_once(source.path(), &dest_dir).expect("second import"));
        assert_eq!(std::fs::read_to_string(dest_dir.join(PROGRESS_FILE_NAME)).expect("kept"), "{}");
    }

    #[test]
    fn nothing_to_copy_and_damaged_files() {
        let source = tempfile::tempdir().expect("temp dir");
        let dest = tempfile::tempdir().expect("temp dir");
        assert!(!import_progress_once(source.path(), dest.path()).expect("no source"));

        std::fs::write(source.path().join(PROGRESS_FILE_NAME), "not json").expect("write source");
        assert!(import_progress_once(source.path(), dest.path()).is_err());
        assert!(!dest.path().join(PROGRESS_FILE_NAME).exists());
    }
}
