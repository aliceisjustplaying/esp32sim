pub mod board;
pub mod bus;
pub mod i2c;
pub mod nat;
pub mod net;
pub mod periph;
pub mod soc;
pub mod wifi;
pub mod crypto {
    pub use esp_periph::crypto::*;
}
pub use esp_soc::{elf, host, image, picture, web, Stop};
pub use soc::{machine, Machine, S3};
