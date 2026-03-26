pub mod pattern;

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::mem;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32Next, Process32Next, MODULEENTRY32, PROCESSENTRY32,
    TH32CS_SNAPMODULE, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, GetForegroundWindow};
use windows::core::PCSTR;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Clone, Default, Debug)]
pub struct ModuleInfo {
    pub base_address: usize,
    pub path: String,
    pub size: usize,
}

pub struct Memory {
    process_handle: HANDLE,
    process_id: u32,
    modules: HashMap<String, ModuleInfo>,
}

// ─── FNV-1a ──────────────────────────────────────────────────────────────────
// Kept here for use by the config layer later. Matches the C++ implementation.

const FNV_BASIS: u64 = 0xCBF29CE484222325;
const FNV_PRIME: u64 = 0x100000001B3;

pub fn fnv1a(s: &str) -> u64 {
    let mut hash = FNV_BASIS;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ─── Memory ──────────────────────────────────────────────────────────────────

impl Memory {
    pub fn new() -> Self {
        Self {
            process_handle: HANDLE::default(),
            process_id: 0,
            modules: HashMap::new(),
        }
    }

    /// Block until `process_name` is found, then open a handle with VM R/W.
    /// Mirrors C++ `CMemory::Initialize`.
    pub fn initialize(&mut self, process_name: &str) {
        loop {
            unsafe {
                let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                };

                let mut entry: PROCESSENTRY32 = mem::zeroed();
                entry.dwSize = mem::size_of::<PROCESSENTRY32>() as u32;

                while Process32Next(snapshot, &mut entry).is_ok() {
                    let name = CStr::from_ptr(entry.szExeFile.as_ptr())
                        .to_string_lossy()
                        .into_owned();

                    if name == process_name {
                        self.process_id = entry.th32ProcessID;
                        self.process_handle = OpenProcess(
                            PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
                            false,
                            entry.th32ProcessID,
                        )
                        .unwrap_or_default();

                        CloseHandle(snapshot).ok();
                        return;
                    }
                }

                CloseHandle(snapshot).ok();
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    /// Read a value of type `T` from the target process at `address`.
    /// Returns zeroed `T` on failure (matches C++ behaviour).
    pub fn read<T: Copy>(&self, address: usize) -> T {
        let mut buffer = mem::MaybeUninit::<T>::zeroed();
        unsafe {
            ReadProcessMemory(
                self.process_handle,
                address as *const _,
                buffer.as_mut_ptr() as *mut _,
                mem::size_of::<T>(),
                None,
            )
            .ok();
            buffer.assume_init()
        }
    }

    /// Follow a pointer chain: dereference `base`, then for each offset
    /// dereference `current + offset`. Returns the final dereferenced value.
    /// Mirrors C++ `ReadMemory<uintptr_t>(base, offsets)`.
    pub fn read_chain(&self, base: usize, offsets: &[usize]) -> usize {
        let mut address = self.read::<usize>(base);
        for &offset in offsets {
            address = self.read::<usize>(address + offset);
        }
        address
    }

    /// Read a null-terminated string (up to MAX_PATH bytes) from the target.
    pub fn read_string(&self, address: usize) -> String {
        let mut buffer = [0u8; 260];
        unsafe {
            ReadProcessMemory(
                self.process_handle,
                address as *const _,
                buffer.as_mut_ptr() as *mut _,
                buffer.len(),
                None,
            )
            .ok();
            CStr::from_bytes_until_nul(&buffer)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    }

    /// Read raw bytes from the target process into `buf`.
    pub fn read_raw(&self, address: usize, buf: &mut [u8]) {
        unsafe {
            ReadProcessMemory(
                self.process_handle,
                address as *const _,
                buf.as_mut_ptr() as *mut _,
                buf.len(),
                None,
            )
            .ok();
        }
    }

    /// Write a value of type `T` to the target process at `address`.
    pub fn write<T: Copy>(&self, address: usize, value: T) {
        unsafe {
            if let Err(e) = WriteProcessMemory(
                self.process_handle,
                address as *mut _,
                &value as *const T as *const _,
                mem::size_of::<T>(),
                None,
            ) {
                eprintln!("[mem] WriteProcessMemory failed @ 0x{:X}: {}", address, e);
            }
        }
    }

    /// Return module info for `module_name`, caching after the first lookup.
    /// Mirrors C++ `CMemory::GetModule`.
    pub fn get_module(&mut self, module_name: &str) -> Option<ModuleInfo> {
        if let Some(info) = self.modules.get(module_name) {
            return Some(info.clone());
        }

        unsafe {
            let snapshot =
                CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, self.process_id).ok()?;

            let mut entry: MODULEENTRY32 = mem::zeroed();
            entry.dwSize = mem::size_of::<MODULEENTRY32>() as u32;

            while Module32Next(snapshot, &mut entry).is_ok() {
                let name = CStr::from_ptr(entry.szModule.as_ptr())
                    .to_string_lossy()
                    .into_owned();

                if name.eq_ignore_ascii_case(module_name) {
                    let path = CStr::from_ptr(entry.szExePath.as_ptr())
                        .to_string_lossy()
                        .into_owned();

                    let info = ModuleInfo {
                        base_address: entry.modBaseAddr as usize,
                        path,
                        size: entry.modBaseSize as usize,
                    };

                    CloseHandle(snapshot).ok();
                    self.modules.insert(module_name.to_string(), info.clone());
                    return Some(info);
                }
            }

            CloseHandle(snapshot).ok();
        }

        None
    }

    /// Returns true when `window_name` is the foreground window.
    /// Mirrors C++ `CMemory::IsWindowInForeground`.
    pub fn is_window_foreground(&self, window_name: &str) -> bool {
        let Ok(cname) = CString::new(window_name) else {
            return false;
        };
        unsafe {
            let Ok(hwnd) = FindWindowA(PCSTR::null(), PCSTR(cname.as_ptr() as *const u8)) else {
                return false;
            };
            if hwnd.0.is_null() {
                return false;
            }
            hwnd == GetForegroundWindow()
        }
    }

    // ─── Pattern scanning ────────────────────────────────────────────────────

    /// Read the full module into a local buffer and scan for `sig`.
    /// Returns the virtual address of the match.
    pub fn pattern_scan(&mut self, module_name: &str, sig: &str) -> Option<usize> {
        let (bytes, base) = self.read_module_bytes(module_name)?;
        pattern::scan(&bytes, base, sig)
    }

    /// Like `pattern_scan` but resolves a RIP-relative operand in the result.
    /// `rva_offset`: byte offset from match start to the 4-byte RVA field.
    /// `rip_offset`: byte offset from match start to end of the instruction.
    pub fn pattern_scan_rip(
        &mut self,
        module_name: &str,
        sig: &str,
        rva_offset: usize,
        rip_offset: usize,
    ) -> Option<usize> {
        let (bytes, base) = self.read_module_bytes(module_name)?;
        let vaddr = pattern::scan(&bytes, base, sig)?;
        let match_offset = vaddr - base;
        pattern::resolve_rip(&bytes, base, match_offset, rva_offset, rip_offset)
    }

    /// Like `pattern_scan` but resolves an absolute encoded address.
    pub fn pattern_scan_abs(
        &mut self,
        module_name: &str,
        sig: &str,
        pre_offset: usize,
        post_offset: usize,
    ) -> Option<usize> {
        let (bytes, base) = self.read_module_bytes(module_name)?;
        let vaddr = pattern::scan(&bytes, base, sig)?;
        let match_offset = vaddr - base;
        pattern::get_absolute(&bytes, base, match_offset, pre_offset, post_offset)
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    /// Read the full bytes of a module into a local Vec, returning (bytes, base_addr).
    fn read_module_bytes(&mut self, module_name: &str) -> Option<(Vec<u8>, usize)> {
        let module = self.get_module(module_name)?;
        let mut bytes = vec![0u8; module.size];

        unsafe {
            ReadProcessMemory(
                self.process_handle,
                module.base_address as *const _,
                bytes.as_mut_ptr() as *mut _,
                module.size,
                None,
            )
            .ok();
        }

        Some((bytes, module.base_address))
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        if !self.process_handle.is_invalid() {
            unsafe {
                CloseHandle(self.process_handle).ok();
            }
        }
    }
}
