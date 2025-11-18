/// Unity Capture backend.
#[cfg(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"))]
pub mod unity_capture;

use crate::image::{Image, ImageSpec};
use std::error::Error;

/// Provides advice on whether an operation should be retried.
pub trait Retry {
    /// Returns `true` if the operation should be retried.
    fn should_retry(&self) -> bool;
}

/// Represents a virtual camera backend.
pub trait Backend {
    /// The [`Connector`] associated with this backend.
    type Connector: Connector;

    /// Returns a connector used to establish a connection with the camera.
    fn connector(&self) -> Self::Connector;
}

/// Represents a stateful camera connection initiator.
pub trait Connector {
    /// Error type returned when `connect` fails.
    type ConnectError: Error + Retry + 'static;

    /// The [`Stream`] associated with this backend.
    type Stream: Stream;

    /// Attempts to establish a connection with the camera.
    fn connect(&mut self) -> Result<Self::Stream, Self::ConnectError>;
}

/// Provides feedback on whether the frame was received by the camera.
pub trait ReceivedFrame {
    /// Returns `true` if the frame was received by the camera.
    fn received_frame(&self) -> bool;
}

/// Represents an open stream to the camera for sending image frames.
pub trait Stream {
    /// Error type returned when sending a frame fails.
    type SendError: Error + Retry + ReceivedFrame + 'static;

    /// Sends a new frame using `f` to write the image data.
    ///
    /// The operation is synchronous, so it returns `Ok` only
    /// after the frame has been received by the camera.
    fn send_with<F>(&mut self, spec: &ImageSpec, f: F) -> Result<(), Self::SendError>
    where
        F: FnOnce(&mut [u8]);

    /// Sends a new frame contained in `image`.
    ///
    /// The operation is synchronous, so it returns `Ok` only
    /// after the frame has been received by the camera.
    fn send(&mut self, image: Image) -> Result<(), Self::SendError> {
        self.send_with(image.spec(), |buf| {
            buf.copy_from_slice(image.data());
        })
    }
}
