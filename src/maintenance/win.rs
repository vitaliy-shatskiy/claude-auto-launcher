//! Win32 for the maintenance family: a streamed SHA-256 (`BCrypt`) and the release-channel GET
//! (`WinHTTP`).
#![expect(unsafe_code, reason = "BCrypt and WinHTTP FFI")]

use std::ffi::c_void;
use std::io::Read as _;
use std::path::Path;
use std::ptr;

use super::release::{FetchFailed, winhttp_failure};
use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
};

const HEX: &[u8; 16] = b"0123456789ABCDEF";

struct Hash(BCRYPT_HASH_HANDLE);

impl Drop for Hash {
    fn drop(&mut self) {
        // SAFETY: a handle BCryptCreateHash returned, destroyed once.
        unsafe { BCryptDestroyHash(self.0) };
    }
}

/// SHA-256 of a file in upper-case hex (`Get-FileHash`), streamed in 1 MiB chunks; `None` when
/// the file cannot be opened or read.
pub(crate) fn sha256_file(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut h: BCRYPT_HASH_HANDLE = ptr::null_mut();
    // SAFETY: the SHA-256 pseudo-handle with no hash object buffer and no secret.
    let st = unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &raw mut h, ptr::null_mut(), 0, ptr::null(), 0, 0) };
    if st < 0 {
        return None;
    }
    let h = Hash(h);
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        let len = u32::try_from(n).ok()?;
        // SAFETY: `buf` holds `n` initialised bytes.
        if unsafe { BCryptHashData(h.0, buf.as_ptr(), len, 0) } < 0 {
            return None;
        }
    }
    let mut out = [0u8; 32];
    // SAFETY: a 32-byte output for SHA-256.
    if unsafe { BCryptFinishHash(h.0, out.as_mut_ptr(), 32, 0) } < 0 {
        return None;
    }
    Some(
        out.iter()
            .flat_map(|b| [char::from(HEX[usize::from(b >> 4)]), char::from(HEX[usize::from(b & 15)])])
            .collect(),
    )
}

struct Net(*mut c_void);

impl Drop for Net {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a WinHTTP handle this module opened, closed once.
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An HTTPS GET of `url` with `timeout_ms` on every phase; the body as text, or why it failed.
pub(crate) fn https_get(url: &str, timeout_ms: i32) -> Result<String, FetchFailed> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| FetchFailed::Reason("only https URLs".into()))?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let last = || winhttp_failure(std::io::Error::last_os_error().raw_os_error().map_or(0, i32::cast_unsigned));
    let agent = wide("claude-auto");
    // SAFETY: valid UTF-16 agent string; no proxy strings with the automatic proxy type.
    let session = Net(unsafe { WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, ptr::null(), ptr::null(), 0) });
    if session.0.is_null() {
        return Err(last());
    }
    // SAFETY: the session handle opened above.
    if unsafe { WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms) } == 0 {
        return Err(last());
    }
    let host_w = wide(host);
    // SAFETY: the session handle and a NUL-terminated host.
    let conn = Net(unsafe { WinHttpConnect(session.0, host_w.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0) });
    if conn.0.is_null() {
        return Err(last());
    }
    let (verb, path_w) = (wide("GET"), wide(&path));
    // SAFETY: the connection handle and NUL-terminated strings; default version, referrer, types.
    let req = Net(unsafe {
        WinHttpOpenRequest(
            conn.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if req.0.is_null() {
        return Err(last());
    }
    // SAFETY: the request handle; no headers and no body.
    if unsafe { WinHttpSendRequest(req.0, ptr::null(), 0, ptr::null(), 0, 0, 0) } == 0 {
        return Err(last());
    }
    // SAFETY: the request handle after a successful send.
    if unsafe { WinHttpReceiveResponse(req.0, ptr::null_mut()) } == 0 {
        return Err(last());
    }
    let mut status: u32 = 0;
    let mut size = 4u32;
    // SAFETY: a u32 buffer of `size` bytes for the numeric status code.
    let ok = unsafe {
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            (&raw mut status).cast(),
            &raw mut size,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(last());
    }
    if status != 200 {
        return Err(FetchFailed::Reason(format!("the release channel answered HTTP {status}")));
    }
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 8192];
    loop {
        let mut read = 0u32;
        let cap = u32::try_from(chunk.len()).unwrap_or(8192);
        // SAFETY: `chunk` has `cap` writable bytes.
        if unsafe { WinHttpReadData(req.0, chunk.as_mut_ptr().cast(), cap, &raw mut read) } == 0 {
            return Err(last());
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(chunk.get(..read as usize).unwrap_or_default());
        if body.len() > 64 * 1024 {
            return Err(FetchFailed::Reason("the release channel answer is too large".into()));
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_known_digest() {
        let p = std::env::temp_dir().join(format!("claude-auto-rs-w4-sha-{}", crate::log::new_run_id()));
        std::fs::write(&p, "abc").unwrap();
        assert_eq!(
            sha256_file(&p).as_deref(),
            Some("BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD")
        );
        std::fs::remove_file(&p).unwrap();
        assert_eq!(sha256_file(&p), None, "a missing file is unknown");
    }

    #[test]
    fn a_non_https_url_is_refused_without_a_request() {
        let r = https_get("http://example.invalid/x", 100);
        assert!(matches!(&r, Err(FetchFailed::Reason(m)) if m == "only https URLs"), "{r:?}");
    }
}
