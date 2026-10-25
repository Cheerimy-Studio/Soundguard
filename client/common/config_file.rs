use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("配置目录无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = parent.join(format!(
        ".client-config-{}-{stamp}-{nonce}.tmp",
        std::process::id()
    ));
    let mut owned = false;
    let result = (|| -> std::io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        owned = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() && owned {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| format!("保存配置失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_keeps_complete_json() {
        let dir = std::env::temp_dir().join(format!("sg-client-config-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("client-config.json");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn concurrent_configurations_do_not_share_temporary_files() {
        let dir = std::env::temp_dir().join(format!("sg-concurrent-config-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let path = dir.join(format!("{i}.json"));
                std::thread::spawn(move || {
                    for n in 0..10 {
                        let value = format!("{{\"file\":{i},\"version\":{n}}}");
                        atomic_write(&path, value.as_bytes()).unwrap();
                    }
                    assert_eq!(
                        fs::read_to_string(path).unwrap(),
                        format!("{{\"file\":{i},\"version\":9}}")
                    );
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
