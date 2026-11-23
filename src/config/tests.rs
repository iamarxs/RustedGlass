use super::*;
use std::fs::File;
use std::io::Write;

fn write_json(dir: &Path, name: &str, body: &str) {
    let mut file = File::create(dir.join(name)).unwrap();
    file.write_all(body.as_bytes()).unwrap();
}

#[test]
fn options_are_not_mistaken_for_a_path() {
    assert_eq!(
        target_path(&["-m".into(), "images/img.png".into()]),
        Some("images/img.png")
    );
    assert_eq!(target_path(&["-m".into(), "-f".into()]), None);
    assert_eq!(target_path(&[]), None);
    assert_eq!(
        target_path(&["--".into(), "-image.png".into()]),
        Some("-image.png")
    );
}

#[test]
fn only_the_user_file_is_loaded_and_cli_can_override_it() {
    let dir = crate::test_path("rg_cfg_test_single_file");
    fs::create_dir_all(&dir).unwrap();
    write_json(&dir, "rgconfig.default.json", r#"{"ZoomSpeed":5}"#);
    write_json(
        &dir,
        "rgconfig.admin.json",
        r#"{"ShowGallery":true,"LockedFeatures":["IG_ZoomIn"]}"#,
    );
    write_json(&dir, CONFIG_USER, r#"{"ShowGallery":true}"#);

    let cfg = Config::load(dir.join(CONFIG_USER), &["-p:ShowGallery=false".to_string()]);
    assert!(!cfg.get_bool("showgallery", true));
    assert!(cfg.get_saved_bool("showgallery", false));
    assert_eq!(cfg.get_f64("ZoomSpeed", 0.0), 0.0);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_old_user_layer_is_dropped_and_reported() {
    let dir = crate::test_path("ig_cfg_test_old");
    fs::create_dir_all(&dir).unwrap();
    write_json(
        &dir,
        CONFIG_USER,
        r#"{"_Metadata":{"ConfigVersion":0.5},"ZoomSpeed":9}"#,
    );

    let path = dir.join(CONFIG_USER);
    let original = fs::read(&path).unwrap();
    let mut cfg = Config::load(path.clone(), &[]);
    assert_eq!(cfg.get_f64("ZoomSpeed", 3.0), 3.0);
    assert_eq!(cfg.incompatible_user_path.as_ref(), Some(&path));
    cfg.set("WindowBounds", serde_json::json!([1, 2, 300, 400]));
    assert!(cfg.save_user().is_err());
    assert_eq!(fs::read(&path).unwrap(), original);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_writes_only_changed_keys_and_keeps_the_rest() {
    let dir = crate::test_path("ig_cfg_test_save");
    fs::create_dir_all(&dir).unwrap();
    write_json(
        &dir,
        CONFIG_USER,
        r#"{"_Metadata":{"ConfigVersion":1.0},"ColorProfile":"srgb","SHOWGALLERY":true,"showgallery":true,"ZoomSpeed":5}"#,
    );
    let mut cfg = Config::load(dir.join(CONFIG_USER), &["-p:ZoomSpeed=9".to_string()]);
    assert!(!cfg.has_changes(), "loading changes nothing");
    cfg.set("showgallery", Value::Bool(true));
    cfg.set("ShowGallery", Value::Bool(false));
    cfg.set("SlideshowInterval", Value::from(9.0));
    cfg.save_user().unwrap();

    let written = read_file(&dir.join(CONFIG_USER)).expect("written user file");
    assert_eq!(
        written["ColorProfile"],
        Value::String("srgb".into()),
        "untouched keys survive"
    );
    assert_eq!(
        written["ZoomSpeed"],
        Value::from(5),
        "a -p: override is not saved"
    );
    assert_eq!(written["ShowGallery"], Value::Bool(false));
    assert!(written.get("SHOWGALLERY").is_none());
    assert!(written.get("showgallery").is_none());
    assert_eq!(written["SlideshowInterval"], Value::from(9.0));

    let reloaded = Config::load(dir.join(CONFIG_USER), &[]);
    assert!(!reloaded.get_bool("ShowGallery", true));
    assert_eq!(reloaded.get_f64("SlideshowInterval", 0.0), 9.0);

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn legacy_zoom_step_preserves_config_precedence() {
    let dir = crate::test_path("legacy-zoom-step");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(CONFIG_USER);
    write_json(&dir, CONFIG_USER, r#"{"ZoomSpeed":-499}"#);
    assert_eq!(Config::load(path.clone(), &[]).zoom_step_percent(), 12.0);

    write_json(
        &dir,
        CONFIG_USER,
        r#"{"ZoomSpeed":-499,"ZoomStepPercent":2}"#,
    );
    let args = ["-p:ZoomSpeed=499".to_string()];
    let mut cfg = Config::load(path.clone(), &args);
    assert_eq!(cfg.zoom_step_percent(), 100.0);
    cfg.set(zoom::STEP_PERCENT_KEY, Value::from(0.1));
    assert_eq!(cfg.zoom_step_percent(), 0.1);
    cfg.save_user().unwrap();
    assert_eq!(Config::load(path, &[]).zoom_step_percent(), 0.1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn saving_a_missing_file_creates_it_with_metadata() {
    let dir = crate::test_path("ig_cfg_test_new");
    fs::create_dir_all(&dir).unwrap();
    fs::remove_file(dir.join(CONFIG_USER)).ok();

    let mut cfg = Config::load(dir.join(CONFIG_USER), &[]);
    cfg.set("ThumbnailSize", Value::from(96u64));
    cfg.save_user().unwrap();

    let written = read_file(&dir.join(CONFIG_USER)).expect("created user file");
    assert_eq!(written["ThumbnailSize"], Value::from(96u64));
    assert_eq!(
        written["_Metadata"]["ConfigVersion"],
        Value::from(SPEC_VERSION as f64)
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn cli_values_keep_their_json_type() {
    let dir = crate::test_path("ig_cfg_test_cli");
    fs::create_dir_all(&dir).unwrap();

    let cfg = Config::load(
        dir.join(CONFIG_USER),
        &[
            "-p:EnableSlideshow=true".to_string(),
            "-p:SlideshowInterval=7".to_string(),
            "-p:ColorProfile=srgb".to_string(),
        ],
    );

    assert!(cfg.get_bool("EnableSlideshow", false));
    assert_eq!(cfg.get_f64("SlideshowInterval", 0.0), 7.0);
    assert_eq!(cfg.get("ColorProfile"), Some(&Value::String("srgb".into())));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_successful_save_clears_the_pending_changes() {
    let dir = crate::test_path("rg_cfg_save_clears_dirty");
    fs::create_dir_all(&dir).unwrap();

    let mut cfg = Config::load(dir.join(CONFIG_USER), &[]);
    cfg.set("ShowGallery", Value::Bool(false));
    assert!(cfg.has_changes());
    cfg.save_user().unwrap();
    assert!(!cfg.has_changes(), "a successful save clears the dirty keys");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_failed_save_keeps_the_changes_pending() {
    let dir = crate::test_path("rg_cfg_failed_save");
    fs::create_dir_all(&dir).unwrap();
    write_json(
        &dir,
        CONFIG_USER,
        r#"{"_Metadata":{"ConfigVersion":0.5}}"#,
    );

    let mut cfg = Config::load(dir.join(CONFIG_USER), &[]);
    cfg.set("ShowGallery", Value::Bool(false));
    assert!(cfg.save_user().is_err());
    assert!(cfg.has_changes(), "a failed save keeps the changes waiting");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_oversized_user_file_is_refused_unparsed() {
    let dir = crate::test_path("rg_cfg_cap");
    fs::create_dir_all(&dir).unwrap();

    let padding = "x".repeat(crate::config::MAX_CONFIG_BYTES as usize);
    write_json(&dir, CONFIG_USER, &format!("{{\"Pad\":\"{padding}\"}}"));
    assert!(
        read_user_file(&dir.join(CONFIG_USER)).is_err(),
        "a file one byte past the cap is refused, not truncated"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_externally_edited_file_is_reread_before_saving() {
    let dir = crate::test_path("rg_cfg_external_edit");
    fs::create_dir_all(&dir).unwrap();
    write_json(&dir, CONFIG_USER, r#"{"ZoomSpeed":5}"#);

    let mut cfg = Config::load(dir.join(CONFIG_USER), &[]);
    // an edit by another program lands between the load and the save
    write_json(&dir, CONFIG_USER, r#"{"ZoomSpeed":9}"#);
    cfg.set("ShowGallery", Value::Bool(false));
    cfg.save_user().unwrap();

    let written = read_file(&dir.join(CONFIG_USER)).expect("written user file");
    assert_eq!(written["ZoomSpeed"], Value::from(9));
    assert_eq!(written["ShowGallery"], Value::Bool(false));
    assert!(!cfg.has_changes(), "the save clears the dirty keys");

    fs::remove_dir_all(&dir).ok();
}
