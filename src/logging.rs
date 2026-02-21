use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tracing_subscriber::fmt::MakeWriter;

// ── DeferredFileWriter ───────────────────────────────────

/// A `MakeWriter` that initially discards writes, then logs to a file
/// once activated via `handle.set_file(path)`.
///
/// This lets us install the tracing subscriber at program start (before
/// we know the VM work directory) and activate file logging later.
#[derive(Clone)]
pub struct DeferredFileWriter {
    inner: Arc<Mutex<Option<File>>>,
}

/// Handle returned by `DeferredFileWriter::new()` — call `set_file` to activate.
#[derive(Clone)]
pub struct DeferredFileHandle {
    inner: Arc<Mutex<Option<File>>>,
}

impl DeferredFileWriter {
    pub fn new() -> (Self, DeferredFileHandle) {
        let inner = Arc::new(Mutex::new(None));
        (
            Self {
                inner: inner.clone(),
            },
            DeferredFileHandle { inner },
        )
    }
}

impl DeferredFileHandle {
    /// Activate the file writer — opens `path` in append mode.
    pub fn set_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        *self.inner.lock().unwrap() = Some(file);
        Ok(())
    }
}

/// Writer returned by `MakeWriter::make_writer` — either writes to the
/// file or discards (when not yet activated).
pub struct DeferredWriter {
    inner: Arc<Mutex<Option<File>>>,
}

impl std::io::Write for DeferredWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(ref mut f) = *guard {
            f.write(buf)
        } else {
            Ok(buf.len()) // discard
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(ref mut f) = *guard {
            f.flush()
        } else {
            Ok(())
        }
    }
}

impl<'a> MakeWriter<'a> for DeferredFileWriter {
    type Writer = DeferredWriter;

    fn make_writer(&'a self) -> Self::Writer {
        DeferredWriter {
            inner: self.inner.clone(),
        }
    }
}

// ── ScriptLogger ─────────────────────────────────────────

/// Logs provisioning script output to a file.
///
/// Creates `<timestamp>_<script-name>_running.log` on construction.
/// Call `finish(success)` to rename to `_ok.log` or `_failed.log`.
pub struct ScriptLogger {
    file: File,
    path: PathBuf,
}

impl ScriptLogger {
    pub fn new(logs_dir: &Path, script_name: &str) -> std::io::Result<Self> {
        fs::create_dir_all(logs_dir)?;
        let ts = utc_timestamp();
        let filename = format!("{ts}_{script_name}_running.log");
        let path = logs_dir.join(&filename);
        let file = File::create(&path)?;
        Ok(Self { file, path })
    }

    /// Write a line to the script log.
    pub fn write_line(&mut self, line: &str) {
        let _ = writeln!(self.file, "{line}");
    }

    /// Finalize the log — renames from `_running` to `_ok` or `_failed`.
    pub fn finish(self, success: bool) {
        let suffix = if success { "ok" } else { "failed" };
        let new_name = self
            .path
            .file_name()
            .and_then(|f| f.to_str())
            .map(|f| f.replace("_running.log", &format!("_{suffix}.log")));
        if let Some(name) = new_name {
            let new_path = self.path.with_file_name(name);
            let _ = fs::rename(&self.path, &new_path);
        }
    }
}

// ── Log rotation ─────────────────────────────────────────

/// Delete oldest log files for a given script name, keeping at most `keep`.
pub fn rotate_logs(logs_dir: &Path, script_name: &str, keep: usize) {
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return;
    };

    let mut matching: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| {
                    f.contains(&format!("_{script_name}_"))
                        && (f.ends_with("_ok.log") || f.ends_with("_failed.log"))
                })
        })
        .collect();

    // Sort by filename (timestamp prefix ensures chronological order)
    matching.sort();

    if matching.len() > keep {
        for old in &matching[..matching.len() - keep] {
            let _ = fs::remove_file(old);
        }
    }
}

// ── Log listing ──────────────────────────────────────────

/// Metadata parsed from a script log filename.
pub struct ScriptLogEntry {
    pub path: PathBuf,
    pub timestamp: String,
    pub script_name: String,
    pub status: String,
}

/// List all completed script logs in the directory, sorted chronologically.
pub fn list_script_logs(logs_dir: &Path) -> Vec<ScriptLogEntry> {
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return Vec::new();
    };

    let mut logs: Vec<ScriptLogEntry> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let fname = path.file_name()?.to_str()?;
            // Format: <timestamp>_<script-name>_<status>.log
            // Skip _running logs
            if fname.ends_with("_running.log") {
                return None;
            }
            if !fname.ends_with(".log") {
                return None;
            }
            let stem = fname.strip_suffix(".log")?;
            // Split on first '_' to get timestamp, then last '_' to get status
            let first_underscore = stem.find('_')?;
            let last_underscore = stem.rfind('_')?;
            if first_underscore == last_underscore {
                return None;
            }
            let timestamp = &stem[..first_underscore];
            let script_name = &stem[first_underscore + 1..last_underscore];
            let status = &stem[last_underscore + 1..];
            Some(ScriptLogEntry {
                path: path.clone(),
                timestamp: timestamp.to_string(),
                script_name: script_name.to_string(),
                status: status.to_string(),
            })
        })
        .collect();

    logs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    logs
}

/// Find the most recent script log, optionally filtering to failed-only.
pub fn latest_script_log(logs_dir: &Path, failed_only: bool) -> Option<PathBuf> {
    let logs = list_script_logs(logs_dir);
    logs.into_iter()
        .rev()
        .find(|e| !failed_only || e.status == "failed")
        .map(|e| e.path)
}

// ── Timestamp helper ─────────────────────────────────────

/// Format the current UTC time as `YYYY-MM-DDTHH-MM-SS` without any
/// external datetime dependency.
pub fn utc_timestamp() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Civil date from unix timestamp
    let days = (secs / 86400) as i64;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Algorithm from Howard Hinnant's civil_from_days
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}-{minutes:02}-{seconds:02}")
}
