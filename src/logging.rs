use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAX_LOG_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub component: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

impl LogEntry {
    pub fn new(level: LogLevel, component: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            component: component.into(),
            message: message.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, ctx: serde_json::Value) -> Self {
        self.context = Some(ctx);
        self
    }
}

/// Ring-buffer logger that persists to JSONL, matching twolebot's pattern
pub struct Logger {
    entries: VecDeque<LogEntry>,
    file_path: std::path::PathBuf,
    writer: Option<BufWriter<File>>,
}

impl Logger {
    pub fn new(file_path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file_path = file_path.as_ref().to_path_buf();

        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut entries = VecDeque::with_capacity(MAX_LOG_ENTRIES);

        if file_path.exists() {
            let file = File::open(&file_path)?;
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
                    if entries.len() >= MAX_LOG_ENTRIES {
                        entries.pop_front();
                    }
                    entries.push_back(entry);
                }
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            entries,
            file_path,
            writer: Some(writer),
        })
    }

    pub fn log(&mut self, entry: LogEntry) {
        if let Some(ref mut writer) = self.writer {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = writeln!(writer, "{}", json);
                let _ = writer.flush();
            }
        }
        if self.entries.len() >= MAX_LOG_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        self.entries.iter().rev().take(limit).cloned().collect()
    }

    pub fn compact(&mut self) -> std::io::Result<()> {
        self.writer = None;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.file_path)?;
        let mut writer = BufWriter::new(file);
        for entry in &self.entries {
            if let Ok(json) = serde_json::to_string(entry) {
                writeln!(writer, "{}", json)?;
            }
        }
        writer.flush()?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        self.writer = Some(BufWriter::new(file));
        Ok(())
    }
}

#[derive(Clone)]
pub struct SharedLogger(Arc<Mutex<Logger>>);

impl SharedLogger {
    pub fn new(file_path: impl AsRef<Path>) -> std::io::Result<Self> {
        Ok(Self(Arc::new(Mutex::new(Logger::new(file_path)?))))
    }

    pub fn log(&self, entry: LogEntry) {
        if let Ok(mut logger) = self.0.lock() {
            logger.log(entry);
        }
    }

    pub fn info(&self, component: impl Into<String>, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Info, component, message));
    }

    pub fn warn(&self, component: impl Into<String>, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Warn, component, message));
    }

    pub fn error(&self, component: impl Into<String>, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Error, component, message));
    }

    pub fn debug(&self, component: impl Into<String>, message: impl Into<String>) {
        self.log(LogEntry::new(LogLevel::Debug, component, message));
    }

    pub fn log_with_context(
        &self,
        level: LogLevel,
        component: impl Into<String>,
        message: impl Into<String>,
        context: serde_json::Value,
    ) {
        self.log(LogEntry::new(level, component, message).with_context(context));
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        self.0.lock().map(|l| l.recent(limit)).unwrap_or_default()
    }
}
