use super::*;

#[test]
fn png_fallback_expands_colors_preserves_sources_and_removes_only_its_temporary_file() {
    let directory = tempdir().unwrap();
    let (sender, cancel) = watch::channel(false);
    for (index, color, bytes, expected) in [
        (
            0,
            ::png::ColorType::Rgba,
            vec![20, 40, 60, 128],
            vec![20, 40, 60],
        ),
        (
            1,
            ::png::ColorType::GrayscaleAlpha,
            vec![80, 128],
            vec![80, 80, 80],
        ),
    ] {
        // Content detection must work even with a non-PNG extension.
        let path = directory.path().join(format!("synthetic-{index}.image"));
        let mut encoder = ::png::Encoder::new(std::fs::File::create(&path).unwrap(), 1, 1);
        encoder.set_color(color);
        encoder.set_depth(::png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&bytes).unwrap();
        writer.finish().unwrap();
        let original = std::fs::read(&path).unwrap();
        let ppm = png::prepare(&path, &cancel).unwrap().unwrap();
        let mut data = b"P6\n1 1\n255\n".to_vec();
        data.extend(expected);
        assert_eq!(std::fs::read(ppm.path()).unwrap(), data);
        let temporary = ppm.path().to_owned();
        drop(ppm);
        assert!(!temporary.exists());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        sender.send_replace(true);
        assert!(png::prepare(&path, &cancel).unwrap().is_none());
        sender.send_replace(false);
    }
    let corrupt = directory.path().join("corrupt.png");
    std::fs::write(&corrupt, b"\x89PNG\r\n\x1a\ninvalid synthetic input").unwrap();
    assert!(png::prepare(&corrupt, &cancel)
        .unwrap_err()
        .contains("image limits"));
    let large = directory.path().join("too-large.png");
    let file = std::fs::File::create(&large).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    assert!(png::prepare(&large, &cancel).is_err());
}
