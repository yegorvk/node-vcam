#![cfg(target_os = "windows")]

use crate::utils::{StrExt, ToUC16StringError};
use snafu::{ResultExt, Snafu};
use std::{
    fmt::{self, Display},
    ptr::NonNull,
};
use windows::Win32::Foundation::HANDLE;
use windows::core::PCWSTR;

// Generic Windows API error type.
type OsError = windows::core::Error;

/// Represents an owned object handle.
// SAFETY: always owns a valid handle.
#[derive(Debug)]
struct Handle(HANDLE);

unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    /// Creates a new `Handle` from a raw handle.
    ///
    /// # Safety
    /// - `handle` must be a valid handle.
    /// - The same handle must not be used elsewhere.
    unsafe fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    fn get(&self) -> HANDLE {
        self.0
    }
}

impl Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{:x}", self.0.0.addr()))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        // SAFETY: `Handle` always contains a valid handle.
        unsafe { CloseHandle(self.0) }
            .unwrap_or_else(|e| eprintln!("Failed to close a handle ({self}): {e}"));
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum OpenMutexError {
    #[snafu(display("Invalid mutex name: `{name}`."))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("Failed to open an existing named mutex: `{name}`."))]
    Os { source: OsError, name: String },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum LockMutexError {
    #[snafu(display("Timed out while trying to acquire a mutex."))]
    Timeout,

    #[snafu(display("Failed to wait on a mutex."))]
    Os { source: OsError },
}

/// Represents a Win32 mutex.
#[derive(Debug)]
pub struct Mutex {
    handle: Handle,
}

impl Mutex {
    /// Opens an existing mutex.
    pub fn open_existing(name: &str) -> Result<Self, OpenMutexError> {
        use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

        let name_wide =
            name.to_u16cstring()
                .with_context(|_| open_mutex_error::InvalidNameSnafu {
                    name: name.to_owned(),
                })?;

        // SAFETY:
        // - Opening a mutex is always safe.
        // - `name_wide` is a nul-terminated UTF-16 string.
        let raw_handle = unsafe {
            OpenMutexW(
                SYNCHRONIZATION_SYNCHRONIZE,
                false,
                PCWSTR::from_raw(name_wide.as_ptr()),
            )
        }
        .with_context(|_| open_mutex_error::OsSnafu {
            name: name.to_owned(),
        })?;

        // SAFETY: `OpenMutexW` always returns a valid handle on success.
        let handle = unsafe { Handle::new(raw_handle) };

        Ok(Mutex { handle })
    }

    /// Acquires the mutex lock, blocking the current thread until it is available or the timeout elapses.
    fn lock(&'_ mut self, should_block: bool) -> Result<MutexGuard<'_>, LockMutexError> {
        // SAFETY: `self.handle` refers to a valid mutex.
        let result = unsafe { wait_for_single_object(self.handle.get(), should_block) };

        match result {
            WaitSingle::Object0 | WaitSingle::Abandoned => Ok(MutexGuard { mutex: self }),
            WaitSingle::Timeout => Err(LockMutexError::Timeout),
            WaitSingle::Failed(err) => Err(err).context(lock_mutex_error::OsSnafu),
        }
    }

    pub fn with_lock<F, B>(&mut self, should_block: bool, f: F) -> Result<B, LockMutexError>
    where
        F: FnOnce() -> B,
    {
        let _guard = self.lock(should_block)?;
        Ok(f())
    }
}

struct MutexGuard<'a> {
    mutex: &'a mut Mutex,
}

impl Drop for MutexGuard<'_> {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::ReleaseMutex;
        // SAFETY: `ReleaseMutex` is always safe to call.
        unsafe { ReleaseMutex(self.mutex.handle.get()) }.unwrap_or_else(|e| {
            eprintln!("Failed to release a mutex ({}): {e}", self.mutex.handle)
        });
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum CreateEventError {
    #[snafu(display("Invalid event name: `{name}`."))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("Failed to create a named event: `{name}`."))]
    Os { source: OsError, name: String },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum OpenEventError {
    #[snafu(display("Invalid event name: `{name}`."))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("Failed to open an existing named event: `{name}`."))]
    Os { source: OsError, name: String },
}

#[derive(Debug, Snafu)]
#[snafu(display("Failed to set (signal) an event."))]
pub struct SetEventError {
    source: OsError,
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum WaitEventError {
    #[snafu(display("Timed out while waiting on an event."))]
    Timeout,

    #[snafu(display("Failed to wait on an event."))]
    Os { source: OsError },
}

/// Represents a Win32 event object.
#[derive(Debug)]
pub struct Event {
    handle: Handle,
}

impl Event {
    /// Creates a new event object.
    pub fn create_new(name: &str) -> Result<Self, CreateEventError> {
        use windows::Win32::System::Threading::CreateEventW;

        let name_wide =
            name.to_u16cstring()
                .with_context(|_| create_event_error::InvalidNameSnafu {
                    name: name.to_owned(),
                })?;

        // SAFETY:
        // - Creating an event object always safe.
        // - `name_wide` is a nul-terminated UTF-16 string.
        let raw_handle =
            unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(name_wide.as_ptr())) }
                .with_context(|_| create_event_error::OsSnafu {
                    name: name.to_owned(),
                })?;

        // SAFETY: `CreateEventW` always returns a valid handle on success.
        let handle = unsafe { Handle::new(raw_handle) };

        Ok(Event { handle })
    }

    /// Opens an existing event object.
    pub fn open_existing(name: &str) -> Result<Self, OpenEventError> {
        use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW};

        let name_wide =
            name.to_u16cstring()
                .with_context(|_| open_event_error::InvalidNameSnafu {
                    name: name.to_owned(),
                })?;

        // SAFETY:
        // - Opening an existing event object always safe.
        // - `name_wide` is a nul-terminated UTF-16 string.
        let raw_handle = unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE,
                false,
                PCWSTR::from_raw(name_wide.as_ptr()),
            )
        }
        .with_context(|_| open_event_error::OsSnafu {
            name: name.to_owned(),
        })?;

        // SAFETY: `CreateEventW` always returns a valid handle on success.
        let handle = unsafe { Handle::new(raw_handle) };

        Ok(Event { handle })
    }

    /// Sets (signals) this event object.
    pub fn set(&self) -> Result<(), SetEventError> {
        use windows::Win32::System::Threading::SetEvent;
        // SAFETY: setting an event object is always safe.
        unsafe { SetEvent(self.handle.get()) }.context(SetEventSnafu)?;
        Ok(())
    }

    /// Blocks the current thread until this event is signaled.
    pub fn wait(&self, should_block: bool) -> Result<(), WaitEventError> {
        // SAFETY: waiting an event object is always safe.
        let result = unsafe { wait_for_single_object(self.handle.0, should_block) };

        match result {
            WaitSingle::Object0 => Ok(()),
            WaitSingle::Timeout => Err(WaitEventError::Timeout),
            WaitSingle::Failed(e) => Err(e).context(wait_event_error::OsSnafu),
            WaitSingle::Abandoned => unreachable!(),
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum OpenFileMappingError {
    #[snafu(display("Invalid file mapping name: `{name}`."))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("Failed to open an existing named file mapping: `{name}`."))]
    Open { source: OsError, name: String },

    #[snafu(display("Failed to map a view of a file mapping."))]
    Map { source: OsError },
}

/// Represents a file mapping object.
#[derive(Debug)]
pub struct FileMapping {
    _handle: Handle,
    // SAFETY:
    // - Must have the same lifetime as `handle`.
    // - Must point to a valid non-empty memory region.
    // - The memory must be valid for reads and writes.
    ptr: NonNull<u8>,
}

impl FileMapping {
    /// Opens an existing file mapping object.
    pub unsafe fn open_existing(name: &str) -> Result<Self, OpenFileMappingError> {
        use windows::Win32::System::Memory::{FILE_MAP_WRITE, OpenFileMappingW};

        let name_wide =
            name.to_u16cstring()
                .with_context(|_| open_file_mapping_error::InvalidNameSnafu {
                    name: name.to_owned(),
                })?;

        // SAFETY: opening a memory mapping object is always safe.
        let raw_handle = unsafe {
            OpenFileMappingW(
                FILE_MAP_WRITE.0,
                false,
                PCWSTR::from_raw(name_wide.as_ptr()),
            )
        }
        .with_context(|_| open_file_mapping_error::OpenSnafu {
            name: name.to_owned(),
        })?;

        // SAFETY: `OpenFileMappingW` always returns a valid handle on success.
        let handle = unsafe { Handle::new(raw_handle) };
        Self::from_handle(handle)
    }

    /// Creates a `FileMapping` from an existing file mapping object handle.
    fn from_handle(handle: Handle) -> Result<Self, OpenFileMappingError> {
        use windows::Win32::System::Memory::{FILE_MAP_WRITE, MapViewOfFile};

        // SAFETY: simply creating a new memory mapping is always safe.
        let ptr = unsafe { MapViewOfFile(handle.get(), FILE_MAP_WRITE, 0, 0, 0) }.Value;

        let ptr = NonNull::new(ptr)
            .ok_or_else(OsError::from_thread)
            .context(open_file_mapping_error::MapSnafu)?
            .cast();

        // SAFETY:
        // - `handle` refers to a valid file mapping object.
        // - `ptr` has the same lifetime as `handle`.
        // - `ptr` points to a valid memory region.
        // - `FILE_MAP_WRITE` ensures that we have read-write access.
        Ok(FileMapping {
            _handle: handle,
            ptr,
        })
    }

    pub fn get_ptr(&self) -> NonNull<u8> {
        self.ptr
    }
}

#[derive(Debug)]
pub struct Lock<T> {
    mutex: Mutex,
    value: T,
}

impl<T> Lock<T> {
    pub fn new(value: T, mutex: Mutex) -> Self {
        Self { mutex, value }
    }

    pub fn with<F, B>(&mut self, should_block: bool, f: F) -> Result<B, LockMutexError>
    where
        F: FnOnce(&mut T) -> B,
    {
        self.mutex.with_lock(should_block, || f(&mut self.value))
    }
}

unsafe fn wait_for_single_object(handle: HANDLE, should_block: bool) -> WaitSingle {
    const WAIT_OBJECT_0: u32 = windows::Win32::Foundation::WAIT_OBJECT_0.0;
    const WAIT_TIMEOUT: u32 = windows::Win32::Foundation::WAIT_TIMEOUT.0;
    const WAIT_ABANDONED: u32 = windows::Win32::Foundation::WAIT_ABANDONED.0;
    const WAIT_FAILED: u32 = windows::Win32::Foundation::WAIT_FAILED.0;

    use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

    let timeout = match should_block {
        true => INFINITE,
        false => 0,
    };

    let result = unsafe { WaitForSingleObject(handle, timeout) }.0;

    match result {
        WAIT_OBJECT_0 => WaitSingle::Object0,
        WAIT_TIMEOUT => WaitSingle::Timeout,
        WAIT_ABANDONED => WaitSingle::Abandoned,
        WAIT_FAILED => WaitSingle::Failed(OsError::from_thread()),
        _ => unreachable!(),
    }
}

#[derive(Debug)]
enum WaitSingle {
    Object0,
    Timeout,
    Abandoned,
    Failed(OsError),
}
