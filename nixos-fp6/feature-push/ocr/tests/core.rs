use fairphone_ocr::{index::Index, ocr};
use std::fs;
#[test]
fn tsv_keeps_diacritics_lines_and_clamps_boxes() {
    let tsv = "level\tpage\tblock\tpar\tline\tword\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t2\t3\t20\t10\t95.2\tȚară\n5\t1\t1\t1\t1\t2\t24\t3\t20\t10\t90\tși\n5\t1\t1\t1\t2\t1\t2\t18\t200\t20\t90\tpâine\n";
    let (words, text) = ocr::parse_tsv(tsv, 100, 30).unwrap();
    assert_eq!(text, "Țară și\npâine");
    assert_eq!(words[2].width, 98);
    assert_eq!(words[2].height, 12);
}
#[test]
fn malformed_coordinates_are_errors() {
    assert!(ocr::parse_tsv("header\n5\t1\t1\t1\t1\t1\t-1\t0\t4\t4\t99\tx", 100, 100).is_err());
}
#[test]
fn index_defaults_private_and_purges_durably() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open(temp.path().join("data")).unwrap();
    index.add("/synthetic.png".into(), "Țară și pâine").unwrap();
    assert!(index.search("țară").is_empty());
    assert!(!temp.path().join("data/index.json").exists());
    index.set_enabled(true).unwrap();
    index.add("/synthetic.png".into(), "Țară și pâine").unwrap();
    assert_eq!(index.search("ȚARĂ").len(), 1);
    assert_eq!(index.search("t\u{0326}ară").len(), 1);
    assert_eq!(
        fs::metadata(temp.path().join("data/index.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    index.set_enabled(false).unwrap();
    let reopened = Index::open(temp.path().join("data")).unwrap();
    assert!(!reopened.enabled());
    assert!(reopened.search("pâine").is_empty());
    assert!(!fs::read_to_string(temp.path().join("data/index.json"))
        .unwrap()
        .contains("pâine"));
}
#[test]
fn index_replaces_entries_and_bounds_history() {
    let temp = tempfile::tempdir().unwrap();
    let mut index = Index::open(temp.path().into()).unwrap();
    index.set_enabled(true).unwrap();
    index.add("/same.png".into(), "old").unwrap();
    index.add("/same.png".into(), "new").unwrap();
    assert!(index.search("old").is_empty());
    assert_eq!(index.search("new").len(), 1);
    for i in 0..501 {
        index.add(format!("/{i}.png"), "synthetic").unwrap();
    }
    assert!(index.search("new").is_empty());
    assert_eq!(index.search("synthetic").len(), 50);
}
#[test]
fn local_inputs_only_and_language_allowlist() {
    assert!(ocr::local_path("https://example.org/image.png").is_err());
    assert!(ocr::local_path("file://remote/image.png").is_err());
    assert!(ocr::languages("eng --config malicious").is_err());
    assert!(ocr::languages("eng+ron").is_ok());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("a b ș.png");
    fs::write(&path, "synthetic").unwrap();
    assert_eq!(
        ocr::local_path(&ocr::file_url(&path).unwrap()).unwrap(),
        path
    );
}
#[test]
fn screenshot_selection_filters_non_images_and_subdirectories() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("one.PNG"), "synthetic").unwrap();
    fs::write(temp.path().join("new.txt"), "synthetic").unwrap();
    fs::create_dir(temp.path().join("nested.png")).unwrap();
    assert_eq!(
        ocr::latest_screenshot(temp.path())
            .unwrap()
            .file_name()
            .unwrap(),
        "one.PNG"
    );
}
#[test]
fn oversized_image_rejected_before_decode() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("large.png");
    // Valid PNG header advertises a huge image; the decoder must never allocate it.
    image::RgbImage::new(4000, 3100).save(&path).unwrap();
    let error = ocr::recognize(path.to_str().unwrap(), "eng", |_, _| {}, || false).unwrap_err();
    assert!(error.to_string().contains("12 megapixel"));
}
