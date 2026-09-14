//! Process snapshot via Toolhelp32, ported from the WPF `ProcessMonitor`
//! (parity slice 8). Binds explicitly to the W exports (`Process32FirstW` /
//! `Process32NextW`) — the default ANSI resolution against a Unicode struct
//! produced mojibake names in the v0.1 C# version. Path lookup uses
//! `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` +
//! `QueryFullProcessImageNameW`, matching the WPF per-process approach.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub path: String,
}

#[cfg(windows)]
mod ffi {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct ProcessEntry32W {
        pub dw_size: u32,
        pub cnt_usage: u32,
        pub th32_process_id: u32,
        pub th32_default_heap_id: usize,
        pub th32_module_id: u32,
        pub cnt_threads: u32,
        pub th32_parent_process_id: u32,
        pub pc_pri_class_base: i32,
        pub dw_flags: u32,
        // szExeFile: MAX_PATH (260) wide chars
        pub sz_exe_file: [u16; 260],
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        #[link_name = "Process32FirstW"]
        pub fn process32_first_w(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        #[link_name = "Process32NextW"]
        pub fn process32_next_w(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        #[link_name = "QueryFullProcessImageNameW"]
        pub fn query_full_process_image_name_w(
            process: *mut c_void,
            flags: u32,
            exe_name: *mut u16,
            size: *mut u32,
        ) -> i32;
    }

    pub const TH32CS_SNAPPROCESS: u32 = 0x2;
    pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
}

/// Snapshots running processes with their executable paths. Processes whose
/// path cannot be queried (access denied, exited) still appear with an empty
/// path — matching the WPF behavior where an empty `ExePath` is legal.
#[cfg(windows)]
pub fn snapshot_processes() -> Vec<ProcessInfo> {
    use ffi::*;
    let mut result = Vec::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot as isize <= 0 {
        return result;
    }

    let mut entry = ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; 260],
    };

    let mut more = unsafe { process32_first_w(snapshot, &mut entry) };
    while more != 0 {
        let pid = entry.th32_process_id;
        let name_len = entry.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(260);
        let name = String::from_utf16_lossy(&entry.sz_exe_file[..name_len]);
        let path = query_process_path(pid);
        if !result.iter().any(|p: &ProcessInfo| p.pid == pid) {
            result.push(ProcessInfo { pid, name, path });
        }
        more = unsafe { process32_next_w(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    result
}

/// Full executable path for one PID (empty when access is denied or the
/// process is gone); shared with the sing-box orphan recovery.
#[cfg(windows)]
pub fn query_process_path(pid: u32) -> String {
    use ffi::*;
    let process = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
    };
    if process.is_null() {
        return String::new();
    }
    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let ok = unsafe {
        query_full_process_image_name_w(process, 0, buffer.as_mut_ptr(), &mut size)
    };
    unsafe { CloseHandle(process) };
    if ok == 0 || size == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..size as usize])
}

/// Full executable path for one PID (unimplemented off Windows).
#[cfg(not(windows))]
pub fn query_process_path(pid: u32) -> String {
    let _ = pid;
    String::new()
}

#[cfg(not(windows))]
pub fn snapshot_processes() -> Vec<ProcessInfo> {
    Vec::new()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn snapshot_includes_current_process() {
        let processes = snapshot_processes();
        assert!(!processes.is_empty(), "process snapshot should not be empty");
        let current = std::process::id();
        assert!(
            processes.iter().any(|p| p.pid == current),
            "snapshot should include the current process (pid {current})"
        );
    }

    #[test]
    fn process_names_have_no_mojibake() {
        let processes = snapshot_processes();
        // If the W-binding were wrong, we'd see UTF-16 surrogate-pair garbage.
        for process in &processes {
            assert!(
                process.name.chars().all(|c| !char::is_control(c)),
                "process name has control characters: {:?}",
                process.name
            );
        }
    }

    #[test]
    fn pids_are_unique() {
        let processes = snapshot_processes();
        let mut pids: Vec<u32> = processes.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        assert_eq!(pids.len(), processes.len(), "duplicate pids in snapshot");
    }
}
