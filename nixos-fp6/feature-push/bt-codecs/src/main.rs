use fp6_bluetooth_audio::{pulse::Pulse, Service, NAME, PATH};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let order = std::env::var("FP6_BT_PREFERRED_CODECS")
        .unwrap_or_else(|_| "ldac,aptx_hd,aac,aptx,sbc_xq,sbc".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let _connection = zbus::blocking::connection::Builder::session()?
        .name(NAME)?
        .serve_at(PATH, Service::new(Pulse, order))?
        .build()?;
    loop {
        std::thread::park();
    }
}
