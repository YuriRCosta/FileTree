use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Text,
    Json,
}

impl Format {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(format!("unknown output format: {value}")),
        }
    }
}

pub struct Output {
    format: Format,
    quiet: bool,
    stdout: Mutex<io::BufWriter<io::Stdout>>,
    stderr: Mutex<io::BufWriter<io::Stderr>>,
}

impl Output {
    pub fn new(format: Format, quiet: bool) -> Self {
        Self {
            format,
            quiet,
            stdout: Mutex::new(io::BufWriter::new(io::stdout())),
            stderr: Mutex::new(io::BufWriter::new(io::stderr())),
        }
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn quiet(&self) -> bool {
        self.quiet
    }

    pub fn text(&self, value: &str) -> io::Result<()> {
        if self.quiet {
            return Ok(());
        }
        self.write_stdout(value)
    }

    pub fn json<T: Serialize>(&self, value: &T) -> io::Result<()> {
        if self.quiet {
            return Ok(());
        }
        let encoded = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
        self.write_stdout(&encoded)
    }

    pub fn value(&self, value: &Value) -> io::Result<()> {
        match self.format {
            Format::Text => self.text_value(value),
            Format::Json => self.json(value),
        }
    }

    pub fn machine<T: Serialize>(&self, value: &T) -> io::Result<()> {
        let encoded = serde_json::to_string(value).map_err(io::Error::other)?;
        self.write_stdout(&encoded)
    }

    pub fn error(&self, message: &str) -> io::Result<()> {
        match self.format {
            Format::Text => self.write_stderr(&format!("filetree: {message}")),
            Format::Json => {
                let encoded = serde_json::to_string(&serde_json::json!({
                    "ok": false,
                    "error": message,
                }))
                .map_err(io::Error::other)?;
                self.write_stderr(&encoded)
            }
        }
    }

    pub fn flush(&self) -> io::Result<()> {
        self.stdout.lock().map_err(poisoned)?.flush()?;
        self.stderr.lock().map_err(poisoned)?.flush()
    }

    fn text_value(&self, value: &Value) -> io::Result<()> {
        match value {
            Value::String(text) => self.text(text),
            _ => self.json(value),
        }
    }

    fn write_stdout(&self, value: &str) -> io::Result<()> {
        let mut stream = self.stdout.lock().map_err(poisoned)?;
        writeln!(stream, "{value}")?;
        stream.flush()
    }

    fn write_stderr(&self, value: &str) -> io::Result<()> {
        let mut stream = self.stderr.lock().map_err(poisoned)?;
        writeln!(stream, "{value}")?;
        stream.flush()
    }
}

fn poisoned<T>(error: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other(error.to_string())
}
