use appdata::DataDir;
use std::fs;

#[test]
fn a_new_data_folder_is_created_with_its_data_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::at(tmp.path().join("DaD Companion")).unwrap();
    assert!(dir.data().is_dir());
    assert_eq!(dir.settings_path(), tmp.path().join("DaD Companion").join("settings.json"));
}

fn dndtools_folder(root: &std::path::Path) -> std::path::PathBuf {
    let data = root.join("DnDTools").join("data");
    fs::create_dir_all(&data).unwrap();
    let db = rusqlite::Connection::open(data.join("market_history.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE listings (id INTEGER); INSERT INTO listings VALUES (7);").unwrap();
    fs::write(data.join("worth_model.json"), r#"{"version": 3}"#).unwrap();
    root.join("DnDTools")
}

#[test]
fn first_run_imports_market_data_and_models_from_dndtools() {
    let tmp = tempfile::tempdir().unwrap();
    let old = dndtools_folder(tmp.path());
    let dir = DataDir::at(tmp.path().join("New")).unwrap();
    let imported = dir.import_from_dndtools(&old).unwrap();
    assert_eq!(imported, vec!["market_history.sqlite", "worth_model.json"]);
    let copy = rusqlite::Connection::open(dir.data().join("market_history.sqlite")).unwrap();
    let rows: i64 = copy.query_row("SELECT COUNT(*) FROM listings", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn the_import_never_overwrites_our_own_data() {
    let tmp = tempfile::tempdir().unwrap();
    let old = dndtools_folder(tmp.path());
    let dir = DataDir::at(tmp.path().join("New")).unwrap();
    fs::write(dir.data().join("worth_model.json"), "mine").unwrap();
    let imported = dir.import_from_dndtools(&old).unwrap();
    assert_eq!(imported, vec!["market_history.sqlite"]);
    assert_eq!(fs::read_to_string(dir.data().join("worth_model.json")).unwrap(), "mine");
}

#[test]
fn no_dndtools_folder_means_nothing_to_import() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::at(tmp.path().join("New")).unwrap();
    assert!(dir.import_from_dndtools(&tmp.path().join("NoSuchFolder")).unwrap().is_empty());
}

fn starter_folder(root: &std::path::Path) -> std::path::PathBuf {
    use std::io::Write;
    let starter = root.join("starter");
    fs::create_dir_all(&starter).unwrap();
    let mut gz = flate2::write::GzEncoder::new(fs::File::create(starter.join("market_history.sqlite.gz")).unwrap(), flate2::Compression::fast());
    gz.write_all(b"starter database bytes").unwrap();
    gz.finish().unwrap();
    fs::write(starter.join("worth_model.json"), r#"{"version": 1}"#).unwrap();
    fs::write(starter.join("market_model.json"), "{}").unwrap();
    starter
}

#[test]
fn a_fresh_install_gets_the_starter_market_data() {
    let tmp = tempfile::tempdir().unwrap();
    let starter = starter_folder(tmp.path());
    let dir = DataDir::at(tmp.path().join("New")).unwrap();
    let installed = dir.install_starter(&starter).unwrap();
    assert_eq!(installed, vec!["market_history.sqlite", "worth_model.json", "market_model.json"]);
    assert_eq!(fs::read(dir.data().join("market_history.sqlite")).unwrap(), b"starter database bytes");
    assert!(dir.install_starter(&starter).unwrap().is_empty(), "a second start installs nothing");
}

#[test]
fn starter_data_never_replaces_imported_or_own_data() {
    let tmp = tempfile::tempdir().unwrap();
    let starter = starter_folder(tmp.path());
    let dir = DataDir::at(tmp.path().join("New")).unwrap();
    fs::write(dir.data().join("market_history.sqlite"), "mine").unwrap();
    assert_eq!(dir.install_starter(&starter).unwrap(), vec!["worth_model.json", "market_model.json"]);
    assert_eq!(fs::read_to_string(dir.data().join("market_history.sqlite")).unwrap(), "mine");
}

#[test]
fn no_starter_folder_means_nothing_to_install() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = DataDir::at(tmp.path().join("New")).unwrap();
    assert!(dir.install_starter(&tmp.path().join("missing")).unwrap().is_empty());
}
