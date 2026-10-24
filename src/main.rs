use std::{process::ExitCode, time::Duration};

fn run() -> lookat::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let watch = match args.as_slice() {
        [] => false,
        [flag] if flag == "--watch" => true,
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "lookat：获取前台窗口所属进程\n\n用法：lookat [--watch]\n  --watch  每 500ms 查询，仅在结果变化时输出；Ctrl+C 退出"
            );
            return Ok(());
        }
        _ => return Err("参数错误，使用 --help 查看用法".into()),
    };
    let mut previous = None;
    loop {
        let current = lookat::foreground_process();
        match current {
            Ok(info) => {
                if previous.as_ref() != Some(&info) {
                    println!(
                        "PID: {}\n进程: {:?}\n路径: {:?}\n窗口: {:?}\n",
                        info.pid, info.name, info.executable, info.window_title
                    );
                    previous = Some(info);
                }
            }
            Err(error) if !watch => return Err(error),
            Err(error) => {
                eprintln!("获取失败: {error}");
                previous = None;
            }
        }
        if !watch {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lookat: {error}");
            ExitCode::FAILURE
        }
    }
}
