use snafu::prelude::*;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::slice;
use windows::Win32::Foundation::HANDLE;
use windows::core::PCWSTR;

use crate::utils::{StrExt, ToUC16StringError};

// Generic Windows API error type.
type Win32Error = windows::core::Error;

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
    unsafe fn new(handle: HANDLE) -> Handle {
        Handle(handle)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        // SAFETY: `Handle` always contains a valid handle.
        unsafe { CloseHandle(self.0) }
            .unwrap_or_else(|e| eprintln!("Failed to close the handle: {e}."));
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum OpenMutexError {
    #[snafu(display("invalid mutex name: `{name}`"))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("failed to open an existing mutex (`{name}`)"))]
    Os { source: Win32Error, name: String },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum LockMutexError {
    #[snafu(display("timed out while acquiring the mutex"))]
    Timeout,

    #[snafu(display("failed to wait on the mutex"))]
    Os { source: Win32Error },
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
    fn lock(&'_ mut self) -> Result<MutexGuard<'_>, LockMutexError> {
        // SAFETY: `self.handle` refers to a valid mutex.
        let result = unsafe { wait_for_single_object(self.handle.0) };

        match result {
            WaitSingle::Object0 | WaitSingle::Abandoned => Ok(MutexGuard { mutex: self }),
            WaitSingle::Timeout => Err(LockMutexError::Timeout),
            WaitSingle::Failed(err) => Err(err).context(lock_mutex_error::OsSnafu),
        }
    }

    pub fn with_lock<F, B>(&mut self, f: F) -> Result<B, LockMutexError>
    where
        F: FnOnce() -> B,
    {
        let _guard = self.lock()?;
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
        unsafe { ReleaseMutex(self.mutex.handle.0) }
            .unwrap_or_else(|e| eprintln!("Failed to release the mutex: {e}."));
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum CreateEventError {
    #[snafu(display("invalid event name: `{name}`"))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("failed to create an event (`{name}`)"))]
    Os { source: Win32Error, name: String },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum OpenEventError {
    #[snafu(display("invalid event name: `{name}`"))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("failed to open an existing event (`{name}`)"))]
    Os { source: Win32Error, name: String },
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to set the event"))]
pub struct SetEventError {
    source: Win32Error,
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum WaitEventError {
    #[snafu(display("timed out while waiting on the event"))]
    Timeout,

    #[snafu(display("failed to wait on the event"))]
    Os { source: Win32Error },
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
        unsafe { SetEvent(self.handle.0) }.context(SetEventSnafu)?;
        Ok(())
    }

    /// Blocks the current thread until this event is signaled.
    pub fn wait(&self) -> Result<(), WaitEventError> {
        // SAFETY: waiting an event object is always safe.
        let result = unsafe { wait_for_single_object(self.handle.0) };

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
    #[snafu(display("invalid file mapping name: `{name}`"))]
    InvalidName {
        source: ToUC16StringError,
        name: String,
    },

    #[snafu(display("failed to open an existing file mapping (`{name}`)"))]
    Open { source: Win32Error, name: String },

    #[snafu(display("failed to map a view of the file mapping"))]
    Map { source: Win32Error },
}

/// Represents a file mapping object.
#[derive(Debug)]
pub struct FileMapping {
    _handle: Handle,
    // SAFETY:
    // - Must have the same lifetime as `handle`.
    // - Must point to a valid memory region of size `size`.
    // - The memory must be valid for reads and writes.
    ptr: NonNull<u8>,
    // SAFETY:
    // - Must not exceed the actual size of the mapping.
    // - Must not exceed `isize::MAX`.
    size: usize,
    _marker: PhantomData<*mut u8>,
}

impl FileMapping {
    /// Opens an existing file mapping object.
    ///
    /// # Safety
    /// - `size` must not exceed the actual size of the mapping object.
    pub unsafe fn open_existing(name: &str, size: usize) -> Result<Self, OpenFileMappingError> {
        use windows::Win32::System::Memory::{FILE_MAP_WRITE, OpenFileMappingW};

        assert!(size > 0, "`size` must not be zero");
        assert!(
            size <= isize::MAX as usize,
            "`size` must not exceed `isize::MAX`"
        );

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

        // SAFETY:
        // - `handle` refers to a valid mapping object.
        // - `size` doesn't exceed the size of the mapping object.
        unsafe { Self::from_handle(handle, size) }
    }

    /// Creates a `FileMapping` from an existing file mapping object handle.
    ///
    /// # Safety
    /// - `size` must not exceed the actual size of the mapping object.
    unsafe fn from_handle(handle: Handle, size: usize) -> Result<Self, OpenFileMappingError> {
        use windows::Win32::System::Memory::{FILE_MAP_WRITE, MapViewOfFile};

        // SAFETY: simply creating a new memory mapping is always safe.
        let ptr = unsafe { MapViewOfFile(handle.0, FILE_MAP_WRITE, 0, 0, 0) }.Value;

        let ptr = NonNull::new(ptr)
            .ok_or_else(Win32Error::from_thread)
            .context(open_file_mapping_error::MapSnafu)?
            .cast();

        // SAFETY:
        // - `handle` refers to a valid file mapping object.
        // - `ptr` has the same lifetime as `handle`.
        // - `ptr` points to a region of size `size`.
        // - `FILE_MAP_WRITE` ensures that we have read-write access.
        // - `size` doesn't exceed the size of the mapping object.
        // - `size` doesn't exceed `isize::MAX`.
        Ok(FileMapping {
            _handle: handle,
            ptr,
            size,
            _marker: PhantomData,
        })
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

    pub fn with_lock<F, B>(&mut self, f: F) -> Result<B, LockMutexError>
    where
        F: FnOnce(&mut T) -> B,
    {
        self.mutex.with_lock(|| f(&mut self.value))
    }
}

#[derive(Debug)]
pub struct SharedMemory {
    mapping: Lock<FileMapping>,
}

impl SharedMemory {
    /// Creates a new `SharedMemory` instance.
    ///
    /// # Safety
    /// - The file mapping object referred to by `mapping` must be accessed
    ///   only via `mapping` by this thread.
    /// - All threads and processes must access that file mapping object only while
    ///   holding (owning) the mutex referred to by `mutex`.
    pub unsafe fn new(mapping: FileMapping, mutex: Mutex) -> Self {
        Self {
            mapping: Lock::new(mapping, mutex),
        }
    }

    pub fn with<F, B>(&mut self, f: F) -> Result<B, LockMutexError>
    where
        F: FnOnce(&mut [u8]) -> B,
    {
        self.mapping.with_lock(|mapping| {
            // SAFETY:
            // - We have exclusive read-write access to the shared memory region.
            // - This memory is "foreign", so initialization doesn't matter.
            // - `ptr` points to memory region of at least `size` bytes.
            // - `size_of::<u8>() * size` doesn't exceed `isize::MAX`.
            let slice = unsafe { slice::from_raw_parts_mut(mapping.ptr.as_ptr(), mapping.size) };
            f(slice)
        })
    }
}

unsafe fn wait_for_single_object(handle: HANDLE) -> WaitSingle {
    const WAIT_OBJECT_0: u32 = windows::Win32::Foundation::WAIT_OBJECT_0.0;
    const WAIT_TIMEOUT: u32 = windows::Win32::Foundation::WAIT_TIMEOUT.0;
    const WAIT_ABANDONED: u32 = windows::Win32::Foundation::WAIT_ABANDONED.0;
    const WAIT_FAILED: u32 = windows::Win32::Foundation::WAIT_FAILED.0;

    use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};

    let result = unsafe { WaitForSingleObject(handle, INFINITE) }.0;

    match result {
        WAIT_OBJECT_0 => WaitSingle::Object0,
        WAIT_TIMEOUT => WaitSingle::Timeout,
        WAIT_ABANDONED => WaitSingle::Abandoned,
        WAIT_FAILED => WaitSingle::Failed(Win32Error::from_thread()),
        _ => unreachable!(),
    }
}

#[derive(Debug)]
enum WaitSingle {
    Object0,
    Timeout,
    Abandoned,
    Failed(Win32Error),
}
