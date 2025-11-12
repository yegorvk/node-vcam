use snafu::prelude::*;
use widestring::U16CString;

pub trait OptionExt<T> {
    fn try_get_or_insert_with<F, E>(&mut self, f: F) -> Result<&mut T, E>
    where
        F: FnOnce() -> Result<T, E>;
}

impl<T> OptionExt<T> for Option<T> {
    fn try_get_or_insert_with<F, E>(&mut self, f: F) -> Result<&mut T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let None = self {
            *self = Some(f()?);
        }

        Ok(unsafe { self.as_mut().unwrap_unchecked() })
    }
}

#[derive(Debug, Snafu)]
#[snafu(
    module,
    display("failed to convert `str` to `U16CStr` (a nul-terminated UTF-16 string)")
)]
pub struct ToUC16StringError {
    source: ContainsNulError,
}

#[derive(Debug, Snafu)]
#[snafu(module, display("UTF-8 string contains a nul byte at position {}", source.nul_position()))]
pub struct ContainsNulError {
    source: widestring::error::ContainsNul<u16>,
}

pub trait StrExt {
    fn to_u16cstring(&self) -> Result<U16CString, ToUC16StringError>;
}

impl StrExt for str {
    fn to_u16cstring(&self) -> Result<U16CString, ToUC16StringError> {
        U16CString::from_str(&self)
            .context(contains_nul_error::ContainsNulSnafu)
            .context(to_uc16_string_error::ToUC16StringSnafu)
    }
}
