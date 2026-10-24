//! 获取当前交互桌面的前台窗口所属进程。
use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod kwin;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcess {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub window_title: String,
}

pub fn foreground_process() -> Result<ForegroundProcess> {
    platform::foreground_process()
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::{fs, process::Command};

    fn command(program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program).args(args).output().map_err(|e| {
            format!("无法运行 {program}: {e}。请安装对应桌面工具并在图形会话中运行。")
        })?;
        if !output.status.success() {
            return Err(format!(
                "{program}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    pub fn foreground_process() -> Result<ForegroundProcess> {
        let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland")
            || std::env::var_os("WAYLAND_DISPLAY").is_some();
        let (pid, title) = if wayland {
            if !std::env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .split(':')
                .any(|s| s.eq_ignore_ascii_case("kde"))
            {
                return Err(
                    "当前 Wayland 桌面暂不支持；已支持 KDE Wayland（直接使用 KWin D-Bus）和 X11。"
                        .into(),
                );
            }
            crate::kwin::query()?
        } else {
            let active = command("xprop", &["-root", "_NET_ACTIVE_WINDOW"])?;
            let id = active
                .split_whitespace()
                .last()
                .filter(|id| id.starts_with("0x") && *id != "0x0")
                .ok_or("没有前台窗口，或窗口管理器不支持 _NET_ACTIVE_WINDOW")?;
            let properties = command(
                "xprop",
                &["-id", id, "_NET_WM_PID", "_NET_WM_NAME", "WM_NAME"],
            )?;
            let pid = properties
                .lines()
                .find(|l| l.starts_with("_NET_WM_PID("))
                .and_then(|l| l.split_once(" = "))
                .ok_or("窗口没有提供进程 PID")?
                .1
                .parse::<u32>()?;
            let title = properties
                .lines()
                .filter(|l| l.starts_with("_NET_WM_NAME(") || l.starts_with("WM_NAME("))
                .find_map(|l| {
                    l.split_once(" = ")
                        .map(|(_, v)| v.trim_matches('"').to_owned())
                })
                .unwrap_or_default();
            (pid, title)
        };
        if pid == 0 {
            return Err("前台窗口没有有效 PID".into());
        }
        let name = fs::read_to_string(format!("/proc/{pid}/comm"))?
            .trim_end()
            .to_owned();
        let executable = fs::read_link(format!("/proc/{pid}/exe")).ok();
        Ok(ForegroundProcess {
            pid,
            name,
            executable,
            window_title: title,
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::c_void;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
        fn GetWindowTextW(window: *mut c_void, text: *mut u16, max: i32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    pub fn foreground_process() -> Result<ForegroundProcess> {
        // SAFETY: 所有缓冲区均有效，长度与实际容量一致；进程句柄在查询后关闭。
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return Err("当前没有前台窗口".into());
            }
            let mut pid = 0;
            if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut title = vec![0u16; 32768];
            let length = GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32);
            let window_title = String::from_utf16_lossy(&title[..length.max(0) as usize]);
            let process = OpenProcess(0x1000, 0, pid);
            if process.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut path = vec![0u16; 32768];
            let mut size = path.len() as u32;
            let success = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size);
            let error = std::io::Error::last_os_error();
            CloseHandle(process);
            if success == 0 {
                return Err(error.into());
            }
            let executable = PathBuf::from(String::from_utf16_lossy(&path[..size as usize]));
            let name = executable
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            Ok(ForegroundProcess {
                pid,
                name,
                executable: Some(executable),
                window_title,
            })
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::*;
    pub fn foreground_process() -> Result<ForegroundProcess> {
        Err("当前操作系统暂不支持；请使用 Linux 或 Windows".into())
    }
}
