// SPDX-License-Identifier: GPL-2.0-only

//! Versioned C ABI; callbacks run on the DSP thread. Only load trusted plugins.
use libloading::Library;
use std::{ffi::CStr, os::raw::c_char, path::Path};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginApi {
    pub abi_version: u32,
    pub name: *const c_char,
    pub process: Option<unsafe extern "C" fn(*const u8, usize)>,
    pub shutdown: Option<unsafe extern "C" fn()>,
}
pub struct Plugin {
    api: PluginApi,
    _library: Library,
    pub name: String,
}
impl Plugin {
    pub fn load(path: &Path) -> Result<Self, String> {
        unsafe {
            let library = Library::new(path).map_err(|e| e.to_string())?;
            let entry = library
                .get::<unsafe extern "C" fn() -> PluginApi>(b"sdr_plugin_v1\0")
                .map_err(|e| e.to_string())?;
            let api = entry();
            if api.abi_version != 1 {
                return Err(crate::tr_format!(
                    "ABI {} incompatible (se necesita 1)",
                    "Incompatible ABI {} (version 1 required)",
                    api.abi_version
                ));
            }
            if api.name.is_null() || api.process.is_none() {
                return Err(crate::tr!(
                    "Complemento sin nombre o callback",
                    "Plugin is missing its name or callback"
                )
                .into());
            }
            let name = CStr::from_ptr(api.name).to_string_lossy().into_owned();
            Ok(Self {
                api,
                _library: library,
                name,
            })
        }
    }
    pub fn process(&self, dibits: &[u8]) {
        if let Some(process) = self.api.process {
            unsafe {
                process(dibits.as_ptr(), dibits.len());
            }
        }
    }
}
impl Drop for Plugin {
    fn drop(&mut self) {
        if let Some(shutdown) = self.api.shutdown {
            unsafe {
                shutdown();
            }
        }
    }
}
