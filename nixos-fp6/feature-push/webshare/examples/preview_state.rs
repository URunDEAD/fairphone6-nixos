//! Synthetic UI fixture only: no network or filesystem access.
fn main() {
    let url = "http://192.168.77.1:8765/s/SyntheticPreviewOnly123";
    let code = qrcode::QrCode::new(url.as_bytes()).unwrap();
    let qr: Vec<Vec<bool>> = (0..code.width())
        .map(|y| {
            (0..code.width())
                .map(|x| code[(x, y)] == qrcode::Color::Dark)
                .collect()
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"active":true,"url":url,"seconds_left":528,"interface":"wlan0","claimed":false,"uploads":true,"text_enabled":true,"files":[{"id":0,"name":"Weekend notes.pdf","size":24184,"downloaded":false},{"id":1,"name":"Garden photo.jpg","size":524288,"downloaded":false}],"pending":[],"text":null,"qr":qr})
    );
}
