use crate::{
    utils::OptionExt,
    win32::{
        CreateEventError, Event, FileMapping, Lock, LockMutexError, Mutex, OpenEventError,
        OpenFileMappingError, OpenMutexError, SetEventError, WaitEventError,
    },
};
use core::slice;
use snafu::{ResultExt, Snafu};
use std::sync::atomic::AtomicI32;
use std::{cell::UnsafeCell, ffi::c_int, ptr::NonNull, sync::atomic::Ordering};

#[derive(Debug, Copy, Clone)]
pub struct ImageSpec {
    extent: ImageExtent,
    format: PixelFormat,
}

impl ImageSpec {
    pub const fn new(extent: ImageExtent, format: PixelFormat) -> Self {
        Self { extent, format }
    }

    const fn size_in_bytes(&self) -> usize {
        self.format.size_in_bytes() * self.extent.area()
    }
}

// Ensures that `ImageSpec::size_in_bytes` never overflows.
const _: usize = ImageSpec::new(ImageExtent::MAX, PixelFormat::Rgba8Linear).size_in_bytes();

#[derive(Debug, Copy, Clone)]
pub enum PixelFormat {
    Rgba8Linear,
}

impl PixelFormat {
    const fn size_in_bytes(&self) -> usize {
        match self {
            PixelFormat::Rgba8Linear => 4 * size_of::<u8>(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ImageExtent {
    width: u32,
    height: u32,
}

impl ImageExtent {
    pub const MAX: Self = Self {
        width: 3840,
        height: 2160,
    };

    pub fn new(width: u32, height: u32) -> Result<Self, ImageExtentError> {
        if width > ImageExtent::MAX.width {
            return Err(ImageExtentError::WidthTooLarge { width });
        }

        if height > ImageExtent::MAX.height {
            return Err(ImageExtentError::HeightTooLarge { height });
        }

        Ok(Self { width, height })
    }

    const fn area(&self) -> usize {
        self.width as usize * self.height as usize
    }
}

// Ensures that `ImageExtent::area` never overflows.
const _: usize = ImageExtent::MAX.area();

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum ImageExtentError {
    #[snafu(display("image width (={width}) must not exceed {}", ImageExtent::MAX.width))]
    WidthTooLarge { width: u32 },

    #[snafu(display("image height (={height}) must not exceed {}", ImageExtent::MAX.height))]
    HeightTooLarge { height: u32 },
}

#[cfg(all(target_os = "windows", target_env = "msvc", target_arch = "x86_64"))]
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
    fn setup(&mut self, spec: &ImageSpec) -> Result<(), ImageTooLargeError> {
        const FORMAT_UINT8: c_int = 0;
        const RESIZE_MODE_LINEAR: c_int = 1;
        const MIRROR_MODE_DISABLED: c_int = 0;
        const FRAME_TIMEOUT: c_int = c_int::MAX - 200;

        let image_size_in_bytes = spec.size_in_bytes();

        if image_size_in_bytes > self.max_size as usize {
            return Err(ImageTooLargeError {
                image_bytes: image_size_in_bytes,
                buffer_size: self.max_size as usize,
            });
        }

        const {
            // Ensures that `spec.size.width` always fits into a `c_int`.
            assert!(ImageExtent::MAX.width <= c_int::MAX as u32);
        }

        let width = spec.extent.width as c_int;

        const {
            // Ensures that `spec.size.height` always fits into a `c_int`.
            assert!(ImageExtent::MAX.height <= c_int::MAX as u32);
        }

        let height = spec.extent.height as c_int;

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
#[snafu(display(
    "The image ({image_bytes} bytes) is too large to fit inside the camera buffer ({buffer_size} bytes)."
))]
pub struct ImageTooLargeError {
    image_bytes: usize,
    buffer_size: usize,
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum AttachError {
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

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum SendError {
    #[snafu(display("The sender isn't attached to any virtual camera."))]
    Detached,

    #[snafu(display("The camera is busy and cannot accept more frames."))]
    Busy,

    #[snafu(display("Failed to wait on the `WANT` event (signaled when a frame is requested)."))]
    WaitWant { source: WaitEventError },

    #[snafu(display("Failed to lock the mutex guarding the shared buffer."))]
    LockMutex { source: LockMutexError },

    #[snafu(transparent)]
    ImageTooLarge { source: ImageTooLargeError },

    #[snafu(display("Failed to signal the `SENT` event."))]
    SignalSent { source: SetEventError },
}

#[derive(Debug, Default)]
struct Detached {
    mutex: Option<Mutex>,
    want_frame: Option<Event>,
    sent_frame: Option<Event>,
}

impl Detached {
    fn attach(&mut self) -> Result<Attached, AttachError> {
        let mutex = self
            .mutex
            .try_get_or_insert_with(|| Mutex::open_existing("UnityCapture_Mutx"))
            .context(attach_error::OpenMutexSnafu)?;

        let mapping = mutex
            .with_lock(true, || {
                self.want_frame.try_get_or_insert_with(|| {
                    Event::create_new("UnityCapture_Want")
                        .context(attach_error::CreateWantEventSnafu)
                })?;

                self.sent_frame.try_get_or_insert_with(|| {
                    Event::open_existing("UnityCapture_Sent")
                        .context(attach_error::OpenSentEventSnafu)
                })?;

                let mapping = unsafe { FileMapping::open_existing("UnityCapture_Data") }
                    .context(attach_error::OpenSharedMemorySnafu)?;

                Ok(mapping)
            })
            .context(attach_error::LockMutexSnafu)??;

        let mutex = self.mutex.take().unwrap();

        let want_frame = self.want_frame.take().unwrap();
        let sent_frame = self.sent_frame.take().unwrap();

        Ok(Attached {
            want_frame,
            sent_frame,
            shared: Lock::new(mapping, mutex),
        })
    }
}

#[derive(Debug)]
struct Attached {
    want_frame: Event,
    sent_frame: Event,
    // SAFETY: the size of the mapping is at least `size_of::<Header>()`.
    shared: Lock<FileMapping>,
}

impl Attached {
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
                    .setup(spec)
                    .map_err(|e| SendError::ImageTooLarge { source: e })?;

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

#[derive(Debug)]
enum State {
    Detached(Detached),
    Attached(Attached),
}

pub struct Image<'a> {
    spec: ImageSpec,
    data: &'a [u8],
}

impl<'a> Image<'a> {
    pub fn new(spec: ImageSpec, data: &'a [u8]) -> Result<Self, CreateImageError> {
        let image_size_in_bytes = spec.size_in_bytes();

        if image_size_in_bytes != data.len() {
            return Err(CreateImageError::DataSizeMismatch {
                expected: image_size_in_bytes,
                actual: data.len(),
            });
        }

        Ok(Self { spec, data })
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum CreateImageError {
    #[snafu(display("Image data size mismatch (expected {expected} bytes, found {actual})."))]
    DataSizeMismatch { expected: usize, actual: usize },
}

#[derive(Debug)]
pub struct Sender {
    state: State,
}

impl Sender {
    pub fn new() -> Sender {
        Sender {
            state: State::Detached(Detached::default()),
        }
    }

    pub fn attach(&mut self) -> Result<(), AttachError> {
        if let State::Detached(state) = &mut self.state {
            self.state = State::Attached(state.attach()?);
        }

        Ok(())
    }

    pub fn send_with<F>(&mut self, spec: &ImageSpec, f: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]),
    {
        match &mut self.state {
            State::Attached(state) => state.send_with(spec, f),
            State::Detached(_) => Err(SendError::Detached),
        }
    }

    pub fn send(&mut self, image: &Image) -> Result<(), SendError> {
        self.send_with(&image.spec, |buf| {
            buf.copy_from_slice(image.data);
        })
    }
}
