mod arch;
mod backend;
mod image;
mod utils;

use crate::{
    backend::{Backend, Connector, ReceivedFrame, Retry, Stream, unity_capture::UnityCapture},
    image::{CreateImageError, Image, ImageExtent, ImageSpec, PixelFormat},
    utils::{DynError, IntoDynResult},
};
use derive_where::derive_where;
use napi_derive::napi;
use snafu::{Report, ResultExt, Snafu};
use std::error::Error;

// NOTE: we are using JSDoc-style documentation for js bindings.

/// Supported virtual camera backends.
#[napi(js_name = "Backend")]
pub enum JsBackend {
    /// Unity Capture backend.
    /// @see https://github.com/schellingb/UnityCapture
    #[cfg(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"))]
    UnityCapture,
}

/// Image formats supported by the camera.
#[napi(js_name = "ImageFormat")]
pub enum JsImageFormat {
    Rgba8Linear,
}

/// Represents a connection to a virtual camera.
#[napi(js_name = "VirtualCamera")]
pub struct JsVirtualCamera {
    inner: Box<dyn VirtualCameraErased>,
}

#[napi]
impl JsVirtualCamera {
    /// Creates a new VirtualCamera instance.
    ///
    /// @param {backend} backend - The backend to use.
    #[napi(constructor)]
    pub fn new(backend: JsBackend) -> Self {
        #[cfg(not(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64")))]
        compile_error!("Currently only Unity Capture backend is supported.");

        let inner = match backend {
            #[cfg(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"))]
            JsBackend::UnityCapture => Box::new(VirtualCamera::new(UnityCapture)),
        };

        Self { inner }
    }

    /// Whether the virtual camera is currently running.
    #[napi(getter)]
    pub fn is_running(&self) -> bool {
        self.inner.is_running()
    }

    /// Starts the virtual camera.
    #[napi]
    pub fn start(&mut self) {
        self.inner.start();
    }

    /// Stops the virtual camera.
    #[napi]
    pub fn stop(&mut self) {
        self.inner.stop();
    }

    /// Submits an image to the virtual camera.
    ///
    /// Returns `true` if the frame was accepted (i.e., a process is consuming
    /// the frames). Returns `false` if no application is reading from the camera.
    ///
    /// @param {number} width - Image width in pixels.
    /// @param {number} height - Image height in pixels.
    /// @param {ImageFormat} - Image data format.
    /// @param {Uint8Array} - Image data in the specified format.
    ///
    /// @returns {boolean} `true` if the frame was accepted (i.e., a process is
    /// consuming frames); `false` otherwise (i.e., no application is reading
    /// from the virtual camera).
    #[napi]
    pub fn send(
        &mut self,
        width: u32,
        height: u32,
        format: JsImageFormat,
        image: &[u8],
    ) -> Result<bool, napi::Error> {
        let extent = ImageExtent::new(width, height).into_napi_result()?;

        let format = match format {
            JsImageFormat::Rgba8Linear => PixelFormat::Rgba8Linear,
        };

        self.inner
            .send(&ImageSpec::new(extent, format), image)
            .into_napi_result()
    }
}

trait BackendExt: Backend {
    type Stream: Stream;
    type ConnectError: Error + Retry + 'static;
    type SendError: Error + Retry + 'static;
}

impl<B: Backend> BackendExt for B {
    type Stream = <B::Connector as Connector>::Stream;
    type ConnectError = <B::Connector as Connector>::ConnectError;
    type SendError = <Self::Stream as Stream>::SendError;
}

enum State<B: BackendExt> {
    Stopped,
    Started(B::Connector),
    Connected(B::Stream),
}

#[derive(Snafu)]
#[derive_where(Debug)]
#[snafu(module)]
enum SendError<B: BackendExt> {
    #[snafu(display("Image data doesn't match the specified format."))]
    ImageFormatError { source: CreateImageError },

    #[snafu(display("The camera isn't running."))]
    Stopped,

    #[snafu(display("Failed to connect to the camera."))]
    Connect { source: B::ConnectError },

    #[snafu(display("Failed to send a frame to the camera."))]
    Send { source: B::SendError },
}

struct VirtualCamera<B: Backend> {
    backend: B,
    state: State<B>,
}

impl<B: Backend> VirtualCamera<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            state: State::Stopped,
        }
    }

    fn is_running(&self) -> bool {
        matches!(self.state, State::Started(_) | State::Connected(_))
    }

    fn start(&mut self) {
        self.state = State::Started(self.backend.connector());
    }

    fn stop(&mut self) {
        self.state = State::Stopped;
    }

    fn send(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<bool, SendError<B>> {
        let image = Image::new(*spec, data).context(send_error::ImageFormatSnafu)?;

        match &mut self.state {
            State::Started(connector) => match connector.connect() {
                Ok(stream) => {
                    self.state = State::Connected(stream);
                }
                Err(e) => {
                    if e.should_retry() {
                        log_debug(&e);
                        return Ok(false);
                    } else {
                        return Err(SendError::Connect { source: e });
                    }
                }
            },
            State::Stopped => return Err(SendError::Stopped),
            _ => (),
        };

        let stream = match &mut self.state {
            State::Connected(stream) => stream,
            _ => unreachable!(),
        };

        stream
            .send(image)
            .map(|_| true)
            .or_else(|e| {
                if e.should_retry() {
                    let received_frame = e.received_frame();
                    log_debug(&e);
                    Ok(received_frame)
                } else {
                    Err(e)
                }
            })
            .context(send_error::SendSnafu)
    }
}

fn log_debug(e: &impl Error) {
    #[cfg(debug_assertions)]
    println!("Debug: {}", Report::from_error(e));
}

trait VirtualCameraErased {
    fn is_running(&self) -> bool;
    fn start(&mut self);
    fn stop(&mut self);
    fn send(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<bool, DynError>;
}

impl<B: Backend + 'static> VirtualCameraErased for VirtualCamera<B> {
    fn is_running(&self) -> bool {
        VirtualCamera::is_running(self)
    }

    fn start(&mut self) {
        VirtualCamera::start(self);
    }

    fn stop(&mut self) {
        VirtualCamera::stop(self);
    }

    fn send(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<bool, DynError> {
        VirtualCamera::send(self, spec, data).into_dyn_result()
    }
}

trait IntoNapiError {
    fn into_napi_error(self) -> napi::Error;
}

impl<T: Error> IntoNapiError for T {
    fn into_napi_error(self) -> napi::Error {
        napi::Error::from_reason(Report::from_error(self).to_string())
    }
}

trait IntoNapiResult<T> {
    fn into_napi_result(self) -> Result<T, napi::Error>;
}

impl<T, E: Error> IntoNapiResult<T> for Result<T, E> {
    fn into_napi_result(self) -> Result<T, napi::Error> {
        self.map_err(|e| e.into_napi_error())
    }
}
