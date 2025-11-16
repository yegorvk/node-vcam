#![cfg(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"))]

mod sender;
mod utils;
mod win32;

use crate::sender::{Image, ImageExtent, ImageSpec, PixelFormat, Sender};
use napi_derive::napi;
use snafu::Report;
use std::error::Error;

#[napi]
#[derive(Debug, Default)]
pub struct VirtualCamera {
    sender: Option<Sender>,
}

#[napi]
impl VirtualCamera {
    /// Creates a new `VirtualCamera` instance.
    ///
    /// The camera is initially stopped.
    #[napi(constructor)]
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns `true` if the camera is currently running, i.e., has been started.
    #[napi(getter)]
    pub fn is_running(&self) -> bool {
        self.sender.is_some()
    }

    /// Starts the camera.
    #[napi]
    pub fn start(&mut self) {
        self.sender = Some(Sender::new());
    }

    /// Stops the camera.
    #[napi]
    pub fn stop(&mut self) {
        self.sender = None;
    }

    /// Sends a new frame to the camera.
    #[napi]
    pub fn send(&mut self, width: u32, height: u32, image: &[u8]) -> Result<bool, napi::Error> {
        let sender = self.sender_mut()?;

        if let Err(e) = sender.attach() {
            println!("Error: {}", Report::from_error(e));
            return Ok(false);
        }

        let image = {
            let extent = ImageExtent::new(width, height).into_napi_result()?;

            Image::new(ImageSpec::new(extent, PixelFormat::Rgba8Linear), image)
                .into_napi_result()?
        };

        sender.send(&image).into_napi_result()?;
        Ok(true)
    }

    fn sender_mut(&mut self) -> Result<&mut Sender, napi::Error> {
        self.sender.as_mut().ok_or_else(|| {
            napi::Error::new(napi::Status::GenericFailure, "The camera isn't running.")
        })
    }
}

trait ErrorExt {
    fn into_napi_error(self) -> napi::Error;
}

impl<T: Error> ErrorExt for T {
    fn into_napi_error(self) -> napi::Error {
        napi::Error::from_reason(Report::from_error(self).to_string())
    }
}

trait ResultExt<T> {
    fn into_napi_result(self) -> Result<T, napi::Error>;
}

impl<T, E: Error> ResultExt<T> for Result<T, E> {
    fn into_napi_result(self) -> Result<T, napi::Error> {
        self.map_err(|e| e.into_napi_error())
    }
}
