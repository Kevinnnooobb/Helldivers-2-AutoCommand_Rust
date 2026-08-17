// 公共基础设施 — 应用目录 / 安全 JSON 写入 / 后台日志
use serde::Serialize;
use std::io;
use std::path::{Path, PathBuf};

/// exe 所在目录（所有应用数据文件与子目录的根）
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 先序列化、成功后才写文件；序列化失败返回 Err 且不触碰现有文件。
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)
}

/// 向 exe 目录下的日志文件追加一行（供后台线程上报错误；失败静默，日志不作为控制流）。
pub fn log_to_file(file_name: &str, line: &str) {
    let path = app_dir().join(file_name);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, format!("{line}\n").as_bytes()));
}
