// SPDX-License-Identifier: LGPL-2.1-or-later
//! Native global lifecycle and default image-path comparison mode.
use std::ffi::c_int;
use std::sync::Mutex;
struct Runtime {
    initialized: bool,
    ignore_case: bool,
    #[cfg(windows)]
    acquired_privileges: bool,
}
static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    initialized: false,
    ignore_case: cfg!(windows),
    #[cfg(windows)]
    acquired_privileges: false,
});
/// Initialize native global defaults. A successful repeated call ignores flags.
/// Linux accepts Windows privilege flags as upstream does. Windows adjusts
/// capture/apply token privileges and honors the strict initialization flags.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_global_init(flags: c_int) -> c_int {
    initialize(flags)
}

pub(crate) fn initialize(flags: c_int) -> c_int {
    let mut state = RUNTIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.initialized {
        return 0;
    }
    crate::engine::diagnostics::ensure_default_sink();
    if flags & !0x3f != 0 || flags & 0x30 == 0x30 {
        return 24;
    }
    #[cfg(windows)]
    if flags & 2 == 0 {
        let capture = windows::capture(true);
        if !capture && flags & 4 != 0 {
            windows::release();
            return 12;
        }
        let apply = windows::apply(true);
        if !apply && flags & 8 != 0 {
            windows::release();
            return 12;
        }
        state.acquired_privileges = true;
    }
    if flags & 0x10 != 0 {
        state.ignore_case = false;
    } else if flags & 0x20 != 0 {
        state.ignore_case = true;
    }
    state.initialized = true;
    0
}
/// Close owned global resources after successful initialization. Repeated cleanup
/// is a no-op; the established default path comparison mode persists.
#[unsafe(no_mangle)]
pub extern "C" fn wimlib_global_cleanup() {
    let mut state = RUNTIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.initialized {
        return;
    }
    #[cfg(windows)]
    if state.acquired_privileges {
        // Upstream keeps its process-global acquisition marker across cleanup;
        // a later DONT_ACQUIRE initialization still releases on cleanup once
        // this library has attempted acquisition in an earlier lifecycle.
        windows::release();
    }
    crate::engine::diagnostics::cleanup();
    state.initialized = false;
}
pub(crate) fn ignore_case() -> bool {
    RUNTIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .ignore_case
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    struct Luid {
        low: u32,
        high: i32,
    }
    #[repr(C)]
    struct TokenPrivileges {
        count: u32,
        luid: Luid,
        attributes: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn SetLastError(error: u32);
        fn GetLastError() -> u32;
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
        fn AdjustTokenPrivileges(
            token: Handle,
            disable: i32,
            state: *const TokenPrivileges,
            length: u32,
            previous: *mut c_void,
            returned: *mut u32,
        ) -> i32;
    }
    fn modify(name: &str, enable: bool) -> bool {
        // These fixed ASCII privilege names fit in the stack buffer; querying
        // a process token does not require temporary heap ownership.
        let mut wide_name = [0u16; 32];
        for (slot, byte) in wide_name.iter_mut().zip(name.bytes()) {
            *slot = u16::from(byte);
        }
        let mut token = std::ptr::null_mut();
        // SAFETY: Writable token output and current-process pseudo handle are valid.
        if unsafe { OpenProcessToken(GetCurrentProcess(), 0x20 | 8, &mut token) } == 0 {
            return false;
        }
        let mut state = TokenPrivileges {
            count: 1,
            luid: Luid { low: 0, high: 0 },
            attributes: if enable { 2 } else { 0 },
        };
        // SAFETY: Terminated privilege name and writable LUID are live.
        let found =
            unsafe { LookupPrivilegeValueW(std::ptr::null(), wide_name.as_ptr(), &mut state.luid) };
        let success = if found == 0 {
            false
        } else {
            // SAFETY: Token is owned here; the exact TOKEN_PRIVILEGES record
            // contains one initialized entry and no previous-state output is requested.
            unsafe {
                SetLastError(0);
                AdjustTokenPrivileges(
                    token,
                    0,
                    &state,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != 0
                    && GetLastError() != 1300
            }
        };
        // SAFETY: Close the process token opened above exactly once.
        unsafe {
            CloseHandle(token);
        }
        success
    }
    pub(super) fn capture(enable: bool) -> bool {
        modify("SeBackupPrivilege", enable) & modify("SeSecurityPrivilege", enable)
    }
    pub(super) fn apply(enable: bool) -> bool {
        modify("SeRestorePrivilege", enable)
            & modify("SeSecurityPrivilege", enable)
            & modify("SeTakeOwnershipPrivilege", enable)
            & modify("SeManageVolumePrivilege", enable)
    }
    pub(super) fn release() {
        capture(false);
        apply(false);
    }
}
