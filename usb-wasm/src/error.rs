use thiserror::Error;

#[derive(Error, Debug)]
pub enum UsbWasmError {
    #[error("rusb error")]
    RusbError(#[from] rusb::Error),
    #[error("device not opened")]
    DeviceNotOpened,
}
