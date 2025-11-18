#![cfg(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"))]

use snafu::{ResultExt, Snafu};

use crate::{
    arch::win32::{
        CreateEventError, Event, FileMapping, Lock, LockMutexError, Mutex, OpenEventError,
        OpenFileMappingError, OpenMutexError, SetEventError, WaitEventError,
    },
    backend::{Backend, Connector, ReceivedFrame, Retry, Stream},
    image::{ImageExtent, ImageSpec, PixelFormat},
    utils::OptionExt,
};
use std::{
    cell::UnsafeCell,
    ffi::c_int,
    ptr::NonNull,
    slice,
    sync::atomic::{AtomicI32, Ordering},
};

type AtomicCInt = AtomicI32;

#[repr(C)]
struct Header {
    max_size: u32,
    // `width` may be read by another process without synchronization.
    // TODO: maybe `UnsafeCell` isn't actually needed here.
    width: UnsafeCell<c_int>,
    height: c_int,
    stride: c_int,
    format: c_int,
    resize_mode: c_int,
    mirror_mode: c_int,
    timeout: c_int,
}

impl Header {
    fn write(&mut self, spec: &ImageSpec) -> Result<(), WriteHeaderError> {
        const FORMAT_UINT8: c_int = 0;
        const RESIZE_MODE_LINEAR: c_int = 1;
        const MIRROR_MODE_DISABLED: c_int = 0;
        const FRAME_TIMEOUT: c_int = c_int::MAX - 200;

        let image_size_in_bytes = spec.size_in_bytes();

        if image_size_in_bytes > self.max_size as usize {
            return Err(WriteHeaderError::ImageTooLarge {
                image_bytes: image_size_in_bytes,
                buffer_size: self.max_size as usize,
            });
        }

        const {
            // Ensures that `extent.width()` always fits into a `c_int`.
            assert!(ImageExtent::MAX.width() <= c_int::MAX as u32);
        }

        let width = spec.extent.width() as c_int;

        const {
            // Ensures that `spec.size.height` always fits into a `c_int`.
            assert!(ImageExtent::MAX.height() <= c_int::MAX as u32);
        }

        let height = spec.extent.height() as c_int;

        let format = match spec.format {
            PixelFormat::Rgba8Linear => FORMAT_UINT8,
        };

        self.write_width(width);
        self.height = height;
        self.stride = width;
        self.format = format;
        self.resize_mode = RESIZE_MODE_LINEAR;
        self.mirror_mode = MIRROR_MODE_DISABLED;
        self.timeout = FRAME_TIMEOUT;

        Ok(())
    }

    fn write_width(&self, value: c_int) {
        let atomic = unsafe { AtomicCInt::from_ptr(self.width.get()) };
        atomic.store(value, Ordering::Release);
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum WriteHeaderError {
    #[snafu(display(
        "The image ({image_bytes} bytes) is too large to fit inside the camera buffer ({buffer_size} bytes)."
    ))]
    ImageTooLarge {
        image_bytes: usize,
        buffer_size: usize,
    },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum ConnectError {
    #[snafu(display("Failed to open the mutex guarding the shared buffer."))]
    OpenMutex { source: OpenMutexError },

    #[snafu(display("Failed to lock the mutex guarding the shared buffer."))]
    LockMutex { source: LockMutexError },

    #[snafu(display(
        "Failed to create the `WANT` event (signaled by the camera when a frame is requested)."
    ))]
    CreateWantEvent { source: CreateEventError },

    #[snafu(display(
        "Failed to open the `SENT` event (signaled by senders when a new frame is available)."
    ))]
    OpenSentEvent { source: OpenEventError },

    #[snafu(display("Failed to open the shared memory buffer (contains frame data)."))]
    OpenSharedMemory { source: OpenFileMappingError },
}

impl Retry for ConnectError {
    fn should_retry(&self) -> bool {
        true
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum SendError {
    #[snafu(display("The camera is busy and cannot accept more frames."))]
    Busy,

    #[snafu(display("Failed to wait on the `WANT` event (signaled when a frame is requested)."))]
    WaitWant { source: WaitEventError },

    #[snafu(display("Failed to lock the mutex guarding the shared buffer."))]
    LockMutex { source: LockMutexError },

    #[snafu(display("Failed to write shared buffer header."))]
    WriteHeader { source: WriteHeaderError },

    #[snafu(display("Failed to signal the `SENT` event."))]
    SignalSent { source: SetEventError },
}

impl Retry for SendError {
    fn should_retry(&self) -> bool {
        matches!(self, SendError::Busy)
    }
}

impl ReceivedFrame for SendError {
    fn received_frame(&self) -> bool {
        false
    }
}

pub struct UnityCapture;

impl Backend for UnityCapture {
    type Connector = UnityCaptureConnector;

    fn connector(&self) -> Self::Connector {
        UnityCaptureConnector::default()
    }
}

#[derive(Debug, Default)]
pub struct UnityCaptureConnector {
    mutex: Option<Mutex>,
    want_frame: Option<Event>,
    sent_frame: Option<Event>,
}

impl Connector for UnityCaptureConnector {
    type ConnectError = ConnectError;
    type Stream = UnityCaptureStream;

    fn connect(&mut self) -> Result<Self::Stream, Self::ConnectError> {
        let mutex = self
            .mutex
            .try_get_or_insert_with(|| Mutex::open_existing("UnityCapture_Mutx"))
            .context(connect_error::OpenMutexSnafu)?;

        let mapping = mutex
            .with_lock(true, || {
                self.want_frame.try_get_or_insert_with(|| {
                    Event::create_new("UnityCapture_Want")
                        .context(connect_error::CreateWantEventSnafu)
                })?;

                self.sent_frame.try_get_or_insert_with(|| {
                    Event::open_existing("UnityCapture_Sent")
                        .context(connect_error::OpenSentEventSnafu)
                })?;

                let mapping = unsafe { FileMapping::open_existing("UnityCapture_Data") }
                    .context(connect_error::OpenSharedMemorySnafu)?;

                Ok(mapping)
            })
            .context(connect_error::LockMutexSnafu)??;

        let mutex = self.mutex.take().unwrap();

        let want_frame = self.want_frame.take().unwrap();
        let sent_frame = self.sent_frame.take().unwrap();

        Ok(UnityCaptureStream {
            want_frame,
            sent_frame,
            shared: Lock::new(mapping, mutex),
        })
    }
}

#[derive(Debug)]
pub struct UnityCaptureStream {
    want_frame: Event,
    sent_frame: Event,
    // SAFETY: the size of the mapping is at least `size_of::<Header>()`.
    shared: Lock<FileMapping>,
}

impl Stream for UnityCaptureStream {
    type SendError = SendError;

    fn send_with<F>(&mut self, spec: &ImageSpec, f: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]),
    {
        self.shared
            .with(true, |mapping| {
                let ptr = mapping.get_ptr();

                let mut header_ptr: NonNull<Header> = ptr.cast();
                assert!(header_ptr.is_aligned());

                // SAFETY:
                // - The mutex guarantees that we have exclusive access to the mapping.
                // - The size of the mapping is at least `size_of::<Header>()`.
                // - `header_ptr` is properly aligned for `Header`.
                // - `Header` can hold arbitrary bit patterns.
                let header = unsafe { header_ptr.as_mut() };

                {
                    header.write_width(1);

                    self.want_frame
                        .wait(false)
                        .or_else(|e| {
                            if let WaitEventError::Timeout = e {
                                Ok(())
                            } else {
                                Err(e)
                            }
                        })
                        .context(send_error::WaitWantSnafu)?;
                }

                header
                    .write(spec)
                    .map_err(|e| SendError::WriteHeader { source: e })?;

                // SAFETY:
                // - `ptr` points points a region of at least `size_of::<Header>()` bytes.
                // - `size_of::<Header>()` cannot exceed `isize::MAX`.
                let buf_ptr = unsafe { ptr.add(size_of::<Header>()) };

                // Invariant: `buf_ptr` points to a region of at least `buf_size` bytes.
                let buf_size = header.max_size as usize;

                // `header.max_size` is a `u32`.
                if const { u32::MAX as usize > isize::MAX as usize } {
                    assert!(buf_size <= isize::MAX as usize);
                }

                // We don't care about the rest of the buffer.
                let buf_size = buf_size.min(spec.size_in_bytes());

                // SAFETY:
                // - The mutex guarantees that we have exclusive access to the mapping.
                // - `header` and `buf` do not overlap.
                // - `buf_ptr` points to a region of at least `buf_size` bytes.
                // - `buf_size` does not exceed `isize::MAX`.
                let buf = unsafe { slice::from_raw_parts_mut(buf_ptr.as_ptr(), buf_size) };
                f(buf);

                // Notify the camera about the new frame.
                self.sent_frame.set().context(send_error::SignalSentSnafu)?;

                Ok::<(), SendError>(())
            })
            .context(send_error::LockMutexSnafu)??;

        Ok(())
    }
}
