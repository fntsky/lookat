//! 临时 KWin 脚本通过会话 D-Bus 回传一个原子窗口快照。
use crate::Result;
use serde::Deserialize;
use std::{
    io::Write,
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};
use zbus::blocking::{Connection, Proxy, connection::Builder};

struct Reply(mpsc::Sender<String>);

#[zbus::interface(name = "org.lookat.Reply")]
impl Reply {
    fn snapshot(&self, payload: String) {
        let _ = self.0.send(payload);
    }
}

struct Client {
    connection: Connection,
    receiver: mpsc::Receiver<String>,
}

impl Client {
    fn new() -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let connection = Builder::session()?
            .serve_at("/org/lookat/Reply", Reply(sender))?
            .build()?;
        Ok(Self {
            connection,
            receiver,
        })
    }

    fn query(&self) -> Result<(u32, String)> {
        let mut file = tempfile::Builder::new()
            .prefix("lookat-")
            .suffix(".js")
            .tempfile()?;
        let name = file
            .path()
            .file_name()
            .ok_or("临时文件名无效")?
            .to_string_lossy()
            .into_owned();
        let destination = self
            .connection
            .unique_name()
            .ok_or("D-Bus 没有分配连接名")?;
        // 目的地址和请求标识使用 JSON 转义，不插入窗口标题等外部数据。
        let script = format!(
            r#"
try {{
    var w = workspace.activeWindow;
    var result = {{ token: {token}, pid: w ? w.pid : 0, title: w ? w.caption : "" }};
    callDBus({destination}, "/org/lookat/Reply", "org.lookat.Reply", "Snapshot", JSON.stringify(result));
}} catch (e) {{
    callDBus({destination}, "/org/lookat/Reply", "org.lookat.Reply", "Snapshot", JSON.stringify({{token: {token}, error: String(e)}}));
}}
"#,
            token = serde_json::to_string(&name)?,
            destination = serde_json::to_string(destination.as_str())?
        );
        file.write_all(script.as_bytes())?;
        file.flush()?;
        let scripting = Proxy::new(
            &self.connection,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )?;
        let id: i32 = scripting.call(
            "loadScript",
            &(file.path().to_str().ok_or("临时路径不是 UTF-8")?, &name),
        )?;
        if id < 0 {
            return Err("KWin 拒绝加载查询脚本".into());
        }
        let loaded = LoadedScript {
            proxy: scripting,
            name: &name,
        };
        let path = format!("/Scripting/Script{id}");
        let runner = Proxy::new(
            &self.connection,
            "org.kde.KWin",
            path.as_str(),
            "org.kde.kwin.Script",
        )?;
        runner.call::<_, _, ()>("run", &())?;
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let result = loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let payload = self
                .receiver
                .recv_timeout(remaining)
                .map_err(|e| format!("等待 KWin 前台窗口快照失败: {e}"))?;
            let snapshot: Snapshot = serde_json::from_str(&payload)?;
            if snapshot.token != name {
                continue;
            }
            if let Some(error) = snapshot.error {
                return Err(format!("KWin 脚本执行失败: {error}").into());
            }
            if snapshot.pid == 0 {
                return Err("当前没有有效的前台窗口进程".into());
            }
            break (snapshot.pid, snapshot.title);
        };
        // 在连接与临时文件释放前显式卸载；错误分支也由 Drop 清理。
        drop(loaded);
        Ok(result)
    }
}

struct LoadedScript<'a> {
    proxy: Proxy<'a>,
    name: &'a str,
}

impl Drop for LoadedScript<'_> {
    fn drop(&mut self) {
        let result = self.proxy.call::<_, _, bool>("unloadScript", &(self.name,));
        if let Err(error) = result {
            eprintln!("lookat: 卸载 KWin 临时脚本失败: {error}");
        }
    }
}

#[derive(Deserialize)]
struct Snapshot {
    token: String,
    #[serde(default)]
    pid: u32,
    #[serde(default)]
    title: String,
    error: Option<String>,
}

pub(crate) fn query() -> Result<(u32, String)> {
    // watch 模式复用同一个 D-Bus 连接，避免重复连接累积总线资源。
    static CLIENT: OnceLock<Mutex<Option<Client>>> = OnceLock::new();
    let mut client = CLIENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "KWin 客户端锁已损坏")?;
    if client.is_none() {
        *client = Some(Client::new()?);
    }
    client.as_ref().ok_or("KWin 客户端初始化失败")?.query()
}
