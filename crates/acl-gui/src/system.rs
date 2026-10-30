use std::sync::{Mutex, OnceLock};
use sysinfo::System;
use tracing::error;
/// Make system information a singleton.
pub static SYSTEM: OnceLock<Option<Mutex<System>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryInfo {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub available: u64,
}
/// Get memory information; if it cannot be read, return the default value.
pub fn memory_info() -> MemoryInfo {
    let sys = SYSTEM.get_or_init(|| match System::new() {
        Ok(system) => Some(Mutex::new(system)),
        Err(err) => {
            error!("Failed to initialize sysinfo: {err}");
            None
        }
    });

    if let Some(mutex) = sys.as_ref()
        && let Ok(mut guard) = mutex.lock()
    {
        guard.refresh_memory();
        MemoryInfo {
            total: guard.total_memory(),
            used: guard.used_memory(),
            free: guard.free_memory(),
            available: guard.available_memory(),
        }
    } else {
        MemoryInfo::default()
    }
}
