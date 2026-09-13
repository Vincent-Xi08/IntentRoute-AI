//! DPAPI password envelope, ported from `AppConfigStore.cs`: protect writes
//! `"dpapi:" + base64(CryptProtectData(utf8(password)), CurrentUser, no
//! entropy)`; unprotect strips the prefix and decrypts, passes legacy
//! plaintext through unchanged (one-time migration), and fails closed on
//! malformed or undecryptable values. Windows-only FFI — the product is
//! Windows-only and the gate runs on windows runners.

const DPAPI_PREFIX: &str = "dpapi:";

#[cfg(windows)]
mod windows_ffi {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct CryptBlob {
        pub cb_data: u32,
        pub pb_data: *mut u8,
    }

    #[repr(C)]
    pub(super) struct PromptStruct {
        pub cb_size: u32,
        pub prompt_flags: u32,
        pub hwnd_app: isize,
        pub sz_prompt: *const u16,
    }

    #[link(name = "crypt32")]
    extern "system" {
        pub(super) fn CryptProtectData(
            data_in: *const CryptBlob,
            data_desc: *const u16,
            optional_entropy: *const CryptBlob,
            reserved: *mut c_void,
            prompt: *const PromptStruct,
            flags: u32,
            data_out: *mut CryptBlob,
        ) -> i32;

        pub(super) fn CryptUnprotectData(
            data_in: *const CryptBlob,
            data_desc_out: *mut *mut u16,
            optional_entropy: *const CryptBlob,
            reserved: *mut c_void,
            prompt: *const PromptStruct,
            flags: u32,
            data_out: *mut CryptBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub(super) fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
}

#[cfg(windows)]
fn crypt_call(protect: bool, input: &[u8]) -> Option<Vec<u8>> {
    use windows_ffi::*;

    let mut in_blob = CryptBlob {
        cb_data: input.len() as u32,
        pb_data: input.as_ptr() as *mut u8,
    };
    let mut out_blob = CryptBlob {
        cb_data: 0,
        pb_data: std::ptr::null_mut(),
    };
    let mut description_out: *mut u16 = std::ptr::null_mut();

    // .NET's ProtectedData maps to these calls with no description, no
    // optional entropy, no reserved/prompt structure, and flags = 0
    // (CurrentUser scope).
    let ok = if protect {
        unsafe {
            CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut out_blob,
            )
        }
    } else {
        unsafe {
            CryptUnprotectData(
                &in_blob,
                &mut description_out,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut out_blob,
            )
        }
    };

    let _ = &mut in_blob;
    if ok == 0 {
        return None;
    }

    if !description_out.is_null() {
        unsafe { LocalFree(description_out.cast()) };
    }
    if out_blob.pb_data.is_null() || out_blob.cb_data == 0 {
        return Some(Vec::new());
    }
    let output =
        unsafe { std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize) }.to_vec();
    unsafe { LocalFree(out_blob.pb_data.cast()) };
    Some(output)
}

/// Produces the persisted envelope for a plaintext password. Empty stays
/// empty; the caller decides at the persistence boundary, exactly like the
/// C# store.
pub fn protect_password(password: &str) -> Option<String> {
    if password.is_empty() {
        return Some(String::new());
    }
    #[cfg(windows)]
    {
        let encrypted = crypt_call(true, password.as_bytes())?;
        use base64::Engine as _;
        Some(format!(
            "{DPAPI_PREFIX}{}",
            base64::engine::general_purpose::STANDARD.encode(encrypted)
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = password;
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum UnprotectError {
    MalformedEnvelope,
    Undecryptable,
}

/// Restores the plaintext password from a stored value. Values without the
/// reserved prefix are legacy plaintext and pass through unchanged (one-time
/// migration); `dpapi:`-prefixed values must decode or the configuration is
/// unusable for this Windows user.
pub fn unprotect_password(stored: &str) -> Result<String, UnprotectError> {
    if stored.is_empty() {
        return Ok(String::new());
    }
    let Some(envelope) = stored.strip_prefix(DPAPI_PREFIX) else {
        return Ok(stored.to_string());
    };

    use base64::Engine as _;
    let encrypted = base64::engine::general_purpose::STANDARD
        .decode(envelope.trim())
        .map_err(|_| UnprotectError::MalformedEnvelope)?;

    #[cfg(windows)]
    {
        let plaintext = crypt_call(false, &encrypted).ok_or(UnprotectError::Undecryptable)?;
        String::from_utf8(plaintext).map_err(|_| UnprotectError::MalformedEnvelope)
    }
    #[cfg(not(windows))]
    {
        let _ = encrypted;
        Err(UnprotectError::Undecryptable)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_the_dpapi_envelope() {
        let envelope = protect_password("hunter2").unwrap();
        assert!(envelope.starts_with("dpapi:"), "envelope shape: {envelope}");
        assert_ne!(envelope, "hunter2");
        assert_eq!(unprotect_password(&envelope).unwrap(), "hunter2");
    }

    #[test]
    fn empty_password_stays_empty() {
        assert_eq!(protect_password("").unwrap(), "");
        assert_eq!(unprotect_password("").unwrap(), "");
    }

    #[test]
    fn legacy_plaintext_passes_through_unchanged() {
        assert_eq!(unprotect_password("plain-secret").unwrap(), "plain-secret");
    }

    #[test]
    fn malformed_envelope_fails_closed() {
        assert_eq!(
            unprotect_password("dpapi:not-base64!!!"),
            Err(UnprotectError::MalformedEnvelope)
        );
        // Valid base64 that is not a DPAPI blob must also fail closed.
        use base64::Engine as _;
        let garbage = base64::engine::general_purpose::STANDARD.encode(b"garbage-bytes");
        assert_eq!(
            unprotect_password(&format!("dpapi:{garbage}")),
            Err(UnprotectError::Undecryptable)
        );
    }

    #[test]
    fn distinct_passwords_produce_distinct_blobs() {
        // DPAPI output is non-deterministic per call; two encryptions of the
        // same secret must still both round-trip to the original.
        let first = protect_password("same-secret").unwrap();
        let second = protect_password("same-secret").unwrap();
        assert_ne!(first, second);
        assert_eq!(unprotect_password(&first).unwrap(), "same-secret");
        assert_eq!(unprotect_password(&second).unwrap(), "same-secret");
    }
}
