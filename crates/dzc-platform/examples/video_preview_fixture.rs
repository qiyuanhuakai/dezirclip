fn main() {
    dzc_platform::video_preview::dispatch();
    let file = std::env::args_os().nth(1).expect("synthetic video fixture path");
    let bytes = std::fs::read(file).unwrap();
    let png = dzc_platform::video_preview::first_frame(&bytes, 192).expect("first frame must decode");
    let image = image::load_from_memory(&png).unwrap().to_rgb8();
    assert!(image.width() <= 192 && image.height() <= 192);
    let center = image.get_pixel(image.width() / 2, image.height() / 2);
    assert!(center[2] > 200 && center[0] < 40 && center[1] < 40, "expected the first blue frame, not a later red frame");
    assert!(dzc_platform::video_preview::first_frame(b"invalid-video", 192).is_none());
    println!("Isolated first-frame decoding and malformed media fallback passed");
}
