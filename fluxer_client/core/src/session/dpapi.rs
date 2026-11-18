//! Token encryption with Windows DPAPI (current-user scope).
//!
//! This is the Rust counterpart of Electron `safeStorage` on Windows, which
//! the official desktop app uses for its boot-state key: the blob can only be
//! decrypted by the same Windows user account. An app-specific entropy value
//! keeps other DPAPI users in the same account from decrypting our blobs by
//! accident (it is not a secret). The caller supplies that entropy; the
//! session store binds it to the account record so a blob only decrypts for
//! the record it was sealed for. Other platforms return
//! [`CryptoError::Unsupported`]; there is no plaintext fallback.

use std::fmt;

use zeroize::Zeroizing;

#[derive(Debug)]
pub enum CryptoError {
    /// No OS-backed secret store on this platform.
    Unsupported,
    /// The input is larger than DPAPI accepts (`u32` length).
    TooLarge,
    /// DPAPI failed; carries the Win32 error code only.
    Os(i32),
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::Unsupported => {
                f.write_str("secure token storage is not supported on this platform")
            }
            CryptoError::TooLarge => f.write_str("data too large for DPAPI"),
            CryptoError::Os(code) => write!(f, "DPAPI call failed (os error {code})"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Encrypts `plaintext` for the current Windows user with `entropy`.
pub fn protect(plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, CryptoError> {
    imp::call(plaintext, entropy, true)
}

/// Decrypts a blob produced by [`protect`] with the same `entropy`. The
/// plaintext is wiped when the returned buffer is dropped.
pub fn unprotect(ciphertext: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    imp::call(ciphertext, entropy, false).map(Zeroizing::new)
}

#[cfg(windows)]
mod imp {
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    use super::CryptoError;

    /// The single DPAPI FFI entry point. `encrypt` selects
    /// `CryptProtectData` vs `CryptUnprotectData`; both share the same
    /// blob-in/blob-out contract.
    pub fn call(input: &[u8], entropy: &[u8], encrypt: bool) -> Result<Vec<u8>, CryptoError> {
        let input_len = u32::try_from(input.len()).map_err(|_| CryptoError::TooLarge)?;
        let entropy_len = u32::try_from(entropy.len()).map_err(|_| CryptoError::TooLarge)?;
        // DPAPI declares the input pointers `*mut` in the blob struct but only
        // reads through them for the in/entropy parameters.
        let data_in = CRYPT_INTEGER_BLOB {
            cbData: input_len,
            pbData: input.as_ptr().cast_mut(),
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_len,
            pbData: entropy.as_ptr().cast_mut(),
        };
        let mut data_out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        // SAFETY: `data_in` and `entropy` point at live, correctly sized
        // slices for the duration of the call and DPAPI only reads them.
        // `data_out` is a valid out-parameter. The description, reserved and
        // prompt pointers are null, which both functions accept;
        // CRYPTPROTECT_UI_FORBIDDEN stops any UI from being shown.
        let ok = unsafe {
            if encrypt {
                CryptProtectData(
                    &data_in,
                    ptr::null(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut data_out,
                )
            } else {
                CryptUnprotectData(
                    &data_in,
                    ptr::null_mut(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut data_out,
                )
            }
        };
        if ok == 0 {
            let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return Err(CryptoError::Os(code));
        }
        if data_out.pbData.is_null() {
            return Ok(Vec::new());
        }

        // SAFETY: on success DPAPI returns `cbData` bytes at `pbData`,
        // allocated with LocalAlloc. We copy them out once, then wipe and
        // release the buffer exactly once with LocalFree; `data_out` is not
        // used afterwards.
        let (output, leaked) = unsafe {
            let len = data_out.cbData as usize;
            let copy = std::slice::from_raw_parts(data_out.pbData, len).to_vec();
            ptr::write_bytes(data_out.pbData, 0, len);
            let leaked = !LocalFree(data_out.pbData.cast()).is_null();
            (copy, leaked)
        };
        if leaked {
            tracing::warn!("LocalFree failed for a DPAPI output buffer (already wiped)");
        }
        Ok(output)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::CryptoError;

    pub fn call(_input: &[u8], _entropy: &[u8], _encrypt: bool) -> Result<Vec<u8>, CryptoError> {
        Err(CryptoError::Unsupported)
    }
}
