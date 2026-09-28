use appdata::Settings;
use std::fs;

#[test]
fn settings_survive_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let mut settings = Settings::load(&path).unwrap();
    assert_eq!(settings.get::<bool>("hoverValues"), None);
    settings.set("hoverValues", false).unwrap();
    settings.set("zoom", 1.1).unwrap();
    let again = Settings::load(&path).unwrap();
    assert_eq!(again.get::<bool>("hoverValues"), Some(false));
    assert_eq!(again.get::<f64>("zoom"), Some(1.1));
}

#[test]
fn a_damaged_settings_file_is_set_aside_not_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    fs::write(&path, "{not json").unwrap();
    let settings = Settings::load(&path).unwrap();
    assert_eq!(settings.get::<bool>("anything"), None);
    assert_eq!(fs::read_to_string(tmp.path().join("settings.json.damaged")).unwrap(), "{not json");
}
