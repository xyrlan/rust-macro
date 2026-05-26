use std::sync::Arc;

use async_trait::async_trait;
use rm_driver::{Driver, DriverError, RawEvent};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Driver impl that consumes RawEvents from stdin (one JSON object per line)
/// and prints events sent toward "the OS" to stdout (one JSON object per line).
/// Used by the CLI to demo the pipeline end-to-end without the real driver.
pub struct StdioDriver {
    stdin: Arc<Mutex<BufReader<tokio::io::Stdin>>>,
    stdout: Arc<Mutex<tokio::io::Stdout>>,
}

impl StdioDriver {
    pub fn new() -> Self {
        Self {
            stdin: Arc::new(Mutex::new(BufReader::new(tokio::io::stdin()))),
            stdout: Arc::new(Mutex::new(tokio::io::stdout())),
        }
    }
}

#[async_trait]
impl Driver for StdioDriver {
    async fn send(&self, event: RawEvent) -> Result<(), DriverError> {
        let mut line = serde_json::to_string(&event).map_err(|e| DriverError::Io(e.to_string()))?;
        line.push('\n');
        let mut out = self.stdout.lock().await;
        out.write_all(line.as_bytes())
            .await
            .map_err(|e| DriverError::Io(e.to_string()))?;
        out.flush()
            .await
            .map_err(|e| DriverError::Io(e.to_string()))?;
        Ok(())
    }

    async fn recv(&self) -> Result<RawEvent, DriverError> {
        let mut buf = String::new();
        let n = {
            let mut r = self.stdin.lock().await;
            r.read_line(&mut buf)
                .await
                .map_err(|e| DriverError::Io(e.to_string()))?
        };
        if n == 0 {
            return Err(DriverError::Closed);
        }
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            return Err(DriverError::Closed);
        }
        serde_json::from_str(trimmed).map_err(|e| DriverError::Io(e.to_string()))
    }
}
