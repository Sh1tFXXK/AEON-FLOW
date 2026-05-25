fn main() {
    match aeon_capture::platform::clipboard_read::probe() {
        Ok(b) => println!("probe OK: {b}"),
        Err(e) => println!("probe ERR: {e}"),
    }
    if let Some(t) = aeon_capture::platform::clipboard_read::read_text() {
        println!("text: {} bytes", t.len());
    } else {
        println!("text: none");
    }
    if let Some(png) = aeon_capture::platform::clipboard_read::read_image_png() {
        println!("image: {} bytes PNG", png.len());
    } else {
        println!("image: none");
    }
}
