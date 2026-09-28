use game_data::{canonical_icon_path, IconPack};
use std::path::PathBuf;

#[test]
fn icon_paths_are_normalised_like_the_pack_stores_them() {
    assert_eq!(canonical_icon_path(r"assets\icons\Weapon\X_5001.png").as_deref(), Some("icons/Weapon/X_5001.webp"));
    assert_eq!(canonical_icon_path("Weapon/X_5001.webp").as_deref(), Some("icons/Weapon/X_5001.webp"));
    assert_eq!(canonical_icon_path("").as_deref(), None);
}

#[test]
fn the_shipped_pack_serves_webp_icons() {
    let pack = IconPack::open(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons.pak")).unwrap();
    assert!(pack.len() > 2000);
    let icon = pack.get("icons/Weapon/HeaterShield_5001.webp").unwrap();
    assert_eq!(&icon[0..4], b"RIFF");
    assert_eq!(&icon[8..12], b"WEBP");
    assert!(pack.get("icons/Weapon/NoSuchItem.webp").is_none());
}
