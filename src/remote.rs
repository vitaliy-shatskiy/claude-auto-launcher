//! The remote/companion path (`Remote.ps1` and the crc branch of `claude-auto.ps1`). PHASE B,
//! worker W4 (PORTING.md). Signatures are final; STUB bodies keep the launch path working.

use crate::types::ChildEnv;

/// What the crc branch launches.
#[derive(Debug, Clone)]
pub struct CrcLaunch<'a> {
    pub argv: &'a [String],
    pub account: &'a str,
    pub show_qr: bool,
    pub child: &'a ChildEnv,
}

/// STUB (W4): `Stop-CompanionServer`; Ok(status line) or Err(reason).
pub fn stop_companion_server() -> Result<String, String> {
    Err("not ported yet".into())
}

/// STUB (W4): the crc branch - companion server, tailnet address, pairing QR, `crc.cmd -- argv`.
/// `None` = crc is not on PATH (the caller prints that line and falls back to plain claude);
/// `Some(code)` = crc ran and exited with `code`.
pub fn launch_via_crc(_req: &CrcLaunch<'_>) -> Option<i32> {
    None
}
