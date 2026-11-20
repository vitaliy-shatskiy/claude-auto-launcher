//! Work counts for the speed budget (`perf/budget.json`): the I/O a process did, read from the OS
//! counters. Exact and independent of machine load, unlike a wall time. Exports `IoCounts`.

use std::os::windows::io::AsRawHandle as _;

/// Read and write calls and their bytes, every file, pipe and console handle included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IoCounts {
    pub read_ops: u64,
    pub read_bytes: u64,
    pub write_ops: u64,
    pub write_bytes: u64,
}

impl IoCounts {
    fn from_os(c: &windows_sys::Win32::System::Threading::IO_COUNTERS) -> Self {
        Self {
            read_ops: c.ReadOperationCount,
            read_bytes: c.ReadTransferCount,
            write_ops: c.WriteOperationCount,
            write_bytes: c.WriteTransferCount,
        }
    }

    /// This process so far; `None` when the OS refuses the query.
    pub fn own() -> Option<Self> {
        crate::sys::io_counters(None).map(|c| Self::from_os(&c))
    }

    /// A child's totals: after `wait`, everything it did. The child must not be dropped yet.
    pub fn of_child(child: &std::process::Child) -> Option<Self> {
        crate::sys::io_counters(Some(child.as_raw_handle())).map(|c| Self::from_os(&c))
    }

    /// The work done between `self` and the later sample `after`.
    #[must_use]
    pub fn since(self, after: Self) -> Self {
        Self {
            read_ops: after.read_ops.saturating_sub(self.read_ops),
            read_bytes: after.read_bytes.saturating_sub(self.read_bytes),
            write_ops: after.write_ops.saturating_sub(self.write_ops),
            write_bytes: after.write_bytes.saturating_sub(self.write_bytes),
        }
    }
}
