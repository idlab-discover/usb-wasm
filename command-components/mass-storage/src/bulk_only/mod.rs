
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::native::*;

mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::wasm::*;