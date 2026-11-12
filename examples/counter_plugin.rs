// SPDX-License-Identifier: GPL-2.0-only

//! rustc --crate-type cdylib examples/counter_plugin.rs -o /tmp/counter.so
use std::{
    ffi::c_char,
    sync::atomic::{AtomicU64, Ordering},
};
static COUNT: AtomicU64 = AtomicU64::new(0);
#[repr(C)]
pub struct PluginApi {
    abi_version: u32,
    name: *const c_char,
    process: Option<unsafe extern "C" fn(*const u8, usize)>,
    shutdown: Option<unsafe extern "C" fn()>,
}
unsafe extern "C" fn process(_: *const u8, count: usize) {
    COUNT.fetch_add(count as u64, Ordering::Relaxed);
}
unsafe extern "C" fn shutdown() {
    eprintln!(
        "Complemento contador: {} dibits",
        COUNT.swap(0, Ordering::Relaxed)
    );
}
#[no_mangle]
pub extern "C" fn sdr_plugin_v1() -> PluginApi {
    PluginApi {
        abi_version: 1,
        name: c"Contador de símbolos".as_ptr(),
        process: Some(process),
        shutdown: Some(shutdown),
    }
}
