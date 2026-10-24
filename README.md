# lookat

Rust 2024 项目，获取当前前台窗口所属进程：PID、进程名、可执行文件路径、窗口标题。
KDE Wayland 后端使用 Rust `zbus` 直接访问会话 D-Bus。前台进程指用户正在交互的窗口所属进程，不是 CPU 占用最高的进程。

```bash
cargo run
cargo run -- --watch
cargo build --release
```

`--watch` 每 500ms 查询，结果变化时输出，Ctrl+C 退出。查询间隔内的短暂切换可能遗漏。

## 平台支持

- Windows：直接调用 Win32 API，须在用户交互会话内运行；目标进程权限不足时返回错误。
- Linux X11：需要 `xprop`，窗口管理器必须提供 EWMH 属性。
- Linux KDE Plasma 6 Wayland：通过 KWin Scripting D-Bus 接口加载临时脚本，读取 `workspace.activeWindow` 的 PID 和标题，并通过 D-Bus 回传。不需要 kdotool、qdbus 或 gdbus；在当前 KDE 图形会话内运行。
- 其他 Wayland 桌面和 macOS 暂不支持，会返回明确错误。

Linux 可执行路径读取失败时仍返回其他信息，路径为 `None`。窗口可能在查询期间关闭或切换；失败时可重试。
X11 的标题保留 xprop 的转义形式。标题与进程名由应用提供，不应当作可信标识。

KWin 查询复用 D-Bus 连接，每次查询后卸载临时脚本并删除临时文件；快照等待超时为 3 秒。窗口 PID 与标题在同一次脚本执行中读取，避免分别查询时切换窗口导致不一致。

需要 KWin 开放 Scripting 接口，且 KWin 与程序能访问相同的临时文件路径。不修改持久桌面配置，也不安装常驻 KWin 插件。

## 作为库调用

```rust,no_run
fn main() -> lookat::Result<()> {
    let process = lookat::foreground_process()?;
    println!("{} {}", process.pid, process.name);
    Ok(())
}
```

参考：[GetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getforegroundwindow)、[GetWindowThreadProcessId](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowthreadprocessid)。

## 验证

在当前 KDE Wayland 会话中已验证真实前台窗口查询和持续监听，能够获取 PID、进程名、路径与标题。监听运行后检查未留下 KWin 脚本对象或临时文件；`cargo fmt --check` 和 `cargo clippy --all-targets -- -D warnings` 通过。Windows 与 X11 后端本次未实机验证。
