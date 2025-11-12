#![cfg(windows)]

mod sender;
mod utils;
mod win32;

use crate::sender::{FrameConfig, Sender};
use napi_derive::napi;
use snafu::Report;
#[napi]
pub const MAX_WIDTH: u32 = sender::MAX_WIDTH;

#[napi]
pub const MAX_HEIGHT: u32 = sender::MAX_HEIGHT;

#[napi]
pub struct Camera {
    sender: Option<Sender>,
    config: FrameConfig,
}

#[napi]
impl Camera {
    #[napi(constructor)]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            sender: None,
            config: FrameConfig::new(width, height),
        }
    }

    #[napi]
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config = FrameConfig::new(width, height);
    }

    #[napi]
    pub fn start(&mut self) {
        self.sender = Some(Sender::new());
    }

    #[napi]
    pub fn stop(&mut self) {
        self.sender = None;
    }

    #[napi]
    pub fn send(&mut self, frame: &[u8]) -> Result<(), napi::Error> {
        let sender = self.sender.as_mut().ok_or_else(|| {
            napi::Error::new(napi::Status::GenericFailure, "the camera isn't running")
        })?;

        sender
            .try_send_with(self.config, |data| {
                data[0..frame.len()].copy_from_slice(frame);
            })
            .or_else(|e| {
                if e.should_retry() {
                    Ok(())
                } else {
                    let message = Report::from_error(e).to_string();
                    Err(napi::Error::new(napi::Status::GenericFailure, message))
                }
            })
    }
}
