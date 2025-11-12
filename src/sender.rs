use crate::{
    utils::OptionExt,
    win32::{
        CreateEventError, Event, FileMapping, LockMutexError, Mutex, OpenEventError,
        OpenFileMappingError, OpenMutexError, SetEventError, SharedMemory, WaitEventError,
    },
};
use snafu::{ResultExt, Snafu};
use std::ffi::c_int;

const MAX_IMAGE_SIZE: usize = 3840 * 2160 * 4 * size_of::<u16>();

pub const MAX_WIDTH: u32 = c_int::MAX as u32;
pub const MAX_HEIGHT: u32 = c_int::MAX as u32;

#[derive(Debug, Copy, Clone)]
pub struct FrameConfig {
    width: u32,
    height: u32,
}

impl FrameConfig {
    pub fn new(width: u32, height: u32) -> FrameConfig {
        if width > c_int::MAX as u32 {
            panic!("`width` must not exceed {}", MAX_WIDTH);
        }

        if height > c_int::MAX as u32 {
            panic!("`height` must not exceed {}", MAX_HEIGHT);
        }

        Self { width, height }
    }
}

#[repr(C)]
struct Header {
    max_size: u32,
    width: c_int,
    height: c_int,
    stride: c_int,
    format: c_int,
    resize_mode: c_int,
    mirror_mode: c_int,
    timeout: c_int,
}

impl Header {
    fn fill(&mut self, width: c_int, height: c_int) {
        const FORMAT_UINT8: c_int = 0;
        const RESIZE_MODE_LINEAR: c_int = 1;
        const MIRROR_MODE_DISABLED: c_int = 0;
        const FRAME_TIMEOUT: c_int = c_int::MAX - 200;

        assert_eq!(self.max_size as usize, MAX_IMAGE_SIZE);

        self.width = width;
        self.height = height;
        self.stride = width;
        self.format = FORMAT_UINT8;
        self.resize_mode = RESIZE_MODE_LINEAR;
        self.mirror_mode = MIRROR_MODE_DISABLED;
        self.timeout = FRAME_TIMEOUT;
    }
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to initialize the sender"))]
    Init { source: InitError },

    #[snafu(display("failed to send a frame to the camera"))]
    Send { source: SendFrameError },
}

impl Error {
    pub fn should_retry(&self) -> bool {
        match &self {
            Error::Init { .. } => true,
            Error::Send { .. } => false,
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum InitError {
    #[snafu(display("failed to open the mutex"))]
    OpenMutex { source: OpenMutexError },

    #[snafu(display("failed to lock the mutex"))]
    LockMutex { source: LockMutexError },

    #[snafu(display("failed to create the `WANT` event"))]
    CreateWantEvent { source: CreateEventError },

    #[snafu(display("failed to open the `SENT` event"))]
    OpenSentEvent { source: OpenEventError },

    #[snafu(display("failed to open the shared memory"))]
    OpenSharedMemory { source: OpenFileMappingError },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum SendFrameError {
    #[snafu(display("failed to wait for the `WANT` event"))]
    WaitWant { source: WaitEventError },

    #[snafu(display("failed to lock the mutex"))]
    LockMutex { source: LockMutexError },

    #[snafu(display("failed to signal (set) the `SENT` event"))]
    SignalSent { source: SetEventError },
}

#[derive(Debug, Default)]
struct Uninit {
    mutex: Option<Mutex>,
    want_frame: Option<Event>,
    sent_frame: Option<Event>,
}

impl Uninit {
    fn try_init(&mut self) -> Result<Ready, InitError> {
        // `[u8]` has 1 byte alignment, so there is no padding.
        const SHARED_DATA_SIZE: usize = size_of::<Header>() + MAX_IMAGE_SIZE;

        let mutex = self
            .mutex
            .try_get_or_insert_with(|| Mutex::open_existing("UnityCapture_Mutx"))
            .context(init_error::OpenMutexSnafu)?;

        let mapping = mutex
            .with_lock(|| {
                self.want_frame.try_get_or_insert_with(|| {
                    Event::create_new("UnityCapture_Want").context(init_error::CreateWantEventSnafu)
                })?;

                self.sent_frame.try_get_or_insert_with(|| {
                    Event::open_existing("UnityCapture_Sent")
                        .context(init_error::OpenSentEventSnafu)
                })?;

                let mapping =
                    unsafe { FileMapping::open_existing("UnityCapture_Data", SHARED_DATA_SIZE) }
                        .context(init_error::OpenSharedMemorySnafu)?;

                Ok(mapping)
            })
            .context(init_error::LockMutexSnafu)??;

        let mutex = self.mutex.take().unwrap();

        let want_frame = self.want_frame.take().unwrap();
        let sent_frame = self.sent_frame.take().unwrap();

        let shared = unsafe { SharedMemory::new(mapping, mutex) };

        Ok(Ready {
            _want_frame: want_frame,
            sent_frame,
            shared,
        })
    }
}

#[derive(Debug)]
struct Ready {
    _want_frame: Event,
    sent_frame: Event,
    shared: SharedMemory,
}

impl Ready {
    fn try_send_with<F>(&mut self, config: FrameConfig, f: F) -> Result<(), SendFrameError>
    where
        F: FnOnce(&mut [u8]),
    {
        self.shared
            .with(|bytes| {
                let (header_bytes, image_bytes) = bytes.split_at_mut(size_of::<Header>());

                let header_ptr: *mut Header = header_bytes.as_mut_ptr().cast();
                assert!(header_bytes.len() == size_of::<Header>() && header_ptr.is_aligned());

                // SAFETY:
                // - `header` isn't null, since `header_bytes` is not empty.
                // - We have exclusive access to `header_bytes`.
                // - `header_bytes.len()` equals `size_of::<Header>()`.
                // - `header_ptr` is properly aligned for `Header`.
                // - `Header` can hold arbitrary bit patterns.
                let header = unsafe { header_ptr.as_mut().unwrap_unchecked() };
                header.fill(config.width as c_int, config.height as c_int);

                f(image_bytes);
            })
            .context(send_frame_error::LockMutexSnafu)?;

        self.sent_frame
            .set()
            .context(send_frame_error::SignalSentSnafu)?;

        Ok(())
    }
}

enum State {
    Uninit(Uninit),
    Ready(Ready),
}

pub struct Sender {
    state: State,
}

impl Sender {
    pub fn new() -> Sender {
        Sender {
            state: State::Uninit(Uninit::default()),
        }
    }

    pub fn try_send_with(
        &mut self,
        config: FrameConfig,
        f: impl FnOnce(&mut [u8]),
    ) -> Result<(), Error> {
        self.ensure_ready()
            .context(InitSnafu)?
            .try_send_with(config, f)
            .context(SendSnafu)
    }

    fn ensure_ready<'a>(&'a mut self) -> Result<&'a mut Ready, InitError> {
        if let State::Uninit(uninit) = &mut self.state {
            self.state = State::Ready(uninit.try_init()?);
        }

        match &mut self.state {
            State::Ready(ready) => Ok(ready),
            State::Uninit(_) => unreachable!(),
        }
    }
}
