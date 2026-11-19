//! Live touch-keyboard preference used by the Windows Settings handler.
//!
//! Windows keeps this session-scoped WNF state separately from TabletTip's
//! registry values. WM_SETTINGCHANGE does not update it. These are internal
//! APIs: resolve them at runtime and refuse unknown payloads instead of
//! reporting a registry-only update as successful.

use std::ffi::c_void;
use std::io;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::core::{s, w};

use super::current_session_id;
use super::transaction::Notification;

// Well-known state used by SettingsHandlers_Keyboard::SaveUserSetting.
// Validated on Windows 11 24H2. Do not substitute a policy or invocation event.
const WNF_TKBN_DESKTOP_MODE_AUTO_IHM: u64 = 0x0F84_0539_A3BC_B035;

type Query = unsafe extern "system" fn(
    *const u64,
    *const c_void,
    *const c_void,
    *mut u32,
    *mut u32,
    *mut u32,
) -> i32;
type Publish = unsafe extern "system" fn(u64, *const c_void, *const u32, u32, *const c_void) -> i32;

#[derive(Clone, Copy)]
struct Api {
    query: Query,
    publish: Publish,
}

impl Api {
    fn load() -> io::Result<Self> {
        let unsupported = || {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows touch keyboard notification API is unavailable",
            )
        };
        // ntdll is already loaded for the process lifetime; do not search for
        // or load a DLL from the application directory.
        unsafe {
            let module = GetModuleHandleW(w!("ntdll.dll")).map_err(|_| unsupported())?;
            let query =
                GetProcAddress(module, s!("NtQueryWnfStateData")).ok_or_else(unsupported)?;
            let publish =
                GetProcAddress(module, s!("RtlPublishWnfStateData")).ok_or_else(unsupported)?;
            Ok(Self {
                query: std::mem::transmute::<unsafe extern "system" fn() -> isize, Query>(query),
                publish: std::mem::transmute::<unsafe extern "system" fn() -> isize, Publish>(
                    publish,
                ),
            })
        }
    }
}

#[derive(Default)]
pub(super) struct WnfNotification {
    api: Option<Api>,
}

impl WnfNotification {
    fn api(&mut self, session_id: u32) -> io::Result<Api> {
        if current_session_id()? != session_id {
            return Err(io::Error::other(
                "touch keyboard notification belongs to a different Windows session",
            ));
        }
        if let Some(api) = self.api {
            return Ok(api);
        }
        let api = Api::load()?;
        self.api = Some(api);
        Ok(api)
    }
}

fn nt_result(status: i32, operation: &str) -> io::Result<()> {
    if status < 0 {
        Err(io::Error::other(format!(
            "touch keyboard WNF {operation} failed: NTSTATUS 0x{:08X}",
            status as u32,
        )))
    } else {
        Ok(())
    }
}

impl Notification for WnfNotification {
    fn read(&mut self, session_id: u32) -> io::Result<u32> {
        let api = self.api(session_id)?;
        let mut size = 4;
        let mut stamp = 0;
        let mut value = 0;
        nt_result(
            unsafe {
                (api.query)(
                    &WNF_TKBN_DESKTOP_MODE_AUTO_IHM,
                    std::ptr::null(),
                    std::ptr::null(),
                    &mut stamp,
                    &mut value,
                    &mut size,
                )
            },
            "query",
        )?;
        if size != 4 || value > 2 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows touch keyboard notification payload is unsupported",
            ));
        }
        Ok(value)
    }

    fn publish(&mut self, session_id: u32, value: u32) -> io::Result<()> {
        if value > 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid touch keyboard mode",
            ));
        }
        let api = self.api(session_id)?;
        nt_result(
            unsafe {
                (api.publish)(
                    WNF_TKBN_DESKTOP_MODE_AUTO_IHM,
                    std::ptr::null(),
                    &value,
                    4,
                    std::ptr::null(),
                )
            },
            "publish",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_recovery_into_a_different_windows_session() {
        let other_session = current_session_id().unwrap().wrapping_add(1);
        let mut notification = WnfNotification::default();
        assert!(notification.read(other_session).is_err());
        assert!(notification.publish(other_session, 0).is_err());
        assert!(notification.api.is_none());
    }
}
