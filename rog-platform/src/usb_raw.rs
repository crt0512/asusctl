use std::time::{Duration, Instant};

use log::{info, warn};
use rusb::{Device, DeviceHandle};

use crate::error::{PlatformError, Result};

/// Minimum time between attempts to reopen a device that stopped responding,
/// so a device that is really gone isn't reset on every write.
const REOPEN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
pub struct USBRaw {
    handle: DeviceHandle<rusb::GlobalContext>,
    id_product: u16,
    last_reopen: Option<Instant>,
}

impl USBRaw {
    pub fn new(id_product: u16) -> Result<Self> {
        Ok(Self {
            handle: Self::open(id_product)?,
            id_product,
            last_reopen: None,
        })
    }

    fn open(id_product: u16) -> Result<DeviceHandle<rusb::GlobalContext>> {
        for device in rusb::devices()?.iter() {
            let device_desc = device.device_descriptor()?;
            if device_desc.vendor_id() == 0x0b05 && device_desc.product_id() == id_product {
                return Self::get_dev_handle(&device);
            }
        }

        Err(PlatformError::MissingFunction(format!(
            "USBRaw dev {} not found",
            id_product
        )))
    }

    fn get_dev_handle(
        device: &Device<rusb::GlobalContext>,
    ) -> Result<DeviceHandle<rusb::GlobalContext>> {
        // We don't expect this ID to ever change
        let device = device.open()?;
        device.reset()?;
        device.set_auto_detach_kernel_driver(true)?;
        device.claim_interface(0)?;
        Ok(device)
    }

    fn write_control(&self, message: &[u8]) -> rusb::Result<usize> {
        self.handle.write_control(
            0x21,  // request_type
            0x09,  // request
            0x35e, // value
            0x00,  // index
            message,
            Duration::from_millis(200),
        )
    }

    /// Writes a control message. If the device stopped responding (e.g. the
    /// kernel reset it and rebound usbhid, dropping our interface claim), the
    /// device is reopened and the write retried once.
    pub fn write_bytes(&mut self, message: &[u8]) -> Result<usize> {
        match self.write_control(message) {
            Err(
                err @ (rusb::Error::Io
                | rusb::Error::NoDevice
                | rusb::Error::NotFound
                | rusb::Error::Pipe),
            ) => {
                if self
                    .last_reopen
                    .is_some_and(|t| t.elapsed() < REOPEN_INTERVAL)
                {
                    return Err(PlatformError::USB(err));
                }
                self.last_reopen = Some(Instant::now());
                warn!(
                    "USBRaw dev {:#06x} write failed ({err}), reopening device",
                    self.id_product
                );
                self.handle = Self::open(self.id_product)?;
                info!("USBRaw dev {:#06x} reopened", self.id_product);
                self.write_control(message).map_err(PlatformError::USB)
            }
            res => res.map_err(PlatformError::USB),
        }
    }
}
