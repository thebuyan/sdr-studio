// SPDX-License-Identifier: GPL-2.0-only

use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn get<T: DeserializeOwned>(settings: &Value, key: &str) -> Option<T> {
    serde_json::from_value(settings.get(key)?.clone()).ok()
}

pub struct Store {
    path: PathBuf,
    saved: Value,
    last_attempt: Instant,
}

impl Store {
    pub fn path() -> Option<PathBuf> {
        let directory = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(directory.join("sdr-studio").join("settings.json"))
    }

    pub fn new(path: PathBuf, saved: Value) -> Self {
        Self {
            path,
            saved,
            last_attempt: Instant::now(),
        }
    }

    pub fn load(path: &Path) -> io::Result<Value> {
        match fs::read(path) {
            Ok(contents) => {
                let settings: Value = serde_json::from_slice(&contents)?;
                if !settings.is_object() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Se esperaba un objeto JSON",
                    ));
                }
                Ok(settings)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Value::Null),
            Err(error) => Err(error),
        }
    }

    pub fn save(&mut self, settings: Value, closing: bool) -> io::Result<bool> {
        if settings == self.saved
            || (!closing && self.last_attempt.elapsed() < Duration::from_secs(1))
        {
            return Ok(false);
        }
        self.last_attempt = Instant::now();
        let directory = self.path.parent().expect("settings path has a parent");
        fs::create_dir_all(directory)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = directory.join(format!(".settings-{}-{stamp}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| {
            serde_json::to_writer_pretty(&mut file, &settings)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result?;
        self.saved = settings;
        Ok(true)
    }
}
