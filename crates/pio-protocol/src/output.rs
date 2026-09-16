//! Bounded connection output. Stored semantic events are never discarded here;
//! closing a slow consumer only ends its delivery connection.
use crate::provider::{Session, err};
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::UnixStream,
    time::{Instant, timeout_at},
};
struct Frame {
    bytes: Vec<u8>,
    offset: usize,
    cursor: Option<(String, Value)>,
}
pub struct Output {
    queue: VecDeque<Frame>,
    pending: usize,
    bound: usize,
    budget: Duration,
    delivered: BTreeMap<String, Value>,
    signals: Option<PathBuf>,
}
impl Output {
    pub fn new(config: &Value) -> Self {
        Self {
            queue: VecDeque::new(),
            pending: 0,
            bound: config["events"]["max_pending_notification_bytes"]
                .as_u64()
                .unwrap_or(2097152) as usize,
            budget: Duration::from_millis(
                config["events"]["backpressure_notice_ms"]
                    .as_u64()
                    .unwrap_or(1000),
            ),
            delivered: BTreeMap::new(),
            signals: config["test_barriers"]["directory"]
                .as_str()
                .map(PathBuf::from),
        }
    }
    pub fn signal(&self, name: &str) -> Result<()> {
        if let Some(dir) = &self.signals {
            std::fs::write(
                dir.join(format!("{name}.signal")),
                b"pio-journal-fake-executor\n",
            )?;
        }
        Ok(())
    }
    pub fn flush_ready(&mut self, stream: &mut UnixStream) -> Result<()> {
        while let Some(frame) = self.queue.front_mut() {
            match stream.try_write(&frame.bytes[frame.offset..]) {
                Ok(0) => bail!("connection closed"),
                Ok(count) => {
                    frame.offset += count;
                    self.pending -= count;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
            if frame.offset == frame.bytes.len() {
                let frame = self.queue.pop_front().unwrap();
                if let Some((id, cursor)) = frame.cursor {
                    self.delivered.insert(id, cursor);
                }
            }
        }
        Ok(())
    }
    async fn drain_until(&mut self, stream: &mut UnixStream, deadline: Instant) -> Result<bool> {
        loop {
            self.flush_ready(stream)?;
            if self.queue.is_empty() {
                return Ok(true);
            }
            if timeout_at(deadline, stream.writable()).await.is_err() {
                return Ok(false);
            }
        }
    }
    pub async fn send(
        &mut self,
        stream: &mut UnixStream,
        value: Value,
        session: &Session,
    ) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        if bytes.len() > session.receive {
            bytes =
                serde_json::to_vec(&err("internal_error", json!({})).frame(value["id"].clone()))?;
        }
        bytes.push(b'\n');
        ensure!(
            bytes.len() <= self.bound,
            "individual frame exceeds output bound"
        );
        self.flush_ready(stream)?;
        if self.pending + bytes.len() > self.bound {
            self.signal("backpressure.stall.started")?;
            let deadline = Instant::now() + self.budget;
            if !self.drain_until(stream, deadline).await? {
                self.signal("backpressure.limit.reached")?;
                if session.feature("core.events.backpressure") {
                    // Drain the partially written frame and every ending notice
                    // within ONE deadline. Never restart it for another subscription.
                    let deadline = Instant::now() + self.budget;
                    if self.drain_until(stream, deadline).await? {
                        for (id, read) in &session.subscriptions {
                            let cursor =
                                self.delivered.get(id).unwrap_or(&read["payload"]["cursor"]);
                            let notice = json!({"jsonrpc":"2.0","method":"core.events.notify","params":{"subscription":id,"items":[],"next_cursor":cursor,"ended":{"reason":"consumer_too_slow"}}});
                            let mut bytes = serde_json::to_vec(&notice)?;
                            bytes.push(b'\n');
                            if !matches!(
                                timeout_at(deadline, stream.write_all(&bytes)).await,
                                Ok(Ok(()))
                            ) {
                                break;
                            }
                        }
                    }
                }
                stream.shutdown().await?;
                self.signal("backpressure.connection.closed")?;
                bail!("consumer_too_slow");
            }
        }
        let cursor = if value["method"] == "core.events.notify" {
            value["params"]["subscription"]
                .as_str()
                .map(|id| (id.to_owned(), value["params"]["next_cursor"].clone()))
        } else {
            None
        };
        self.pending += bytes.len();
        self.queue.push_back(Frame {
            bytes,
            offset: 0,
            cursor,
        });
        self.flush_ready(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    #[tokio::test]
    async fn draining_consumer_preserves_all_frames_in_order() {
        let (mut server, client) = UnixStream::pair().unwrap();
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(client);
            let mut line = String::new();
            for id in 0..500 {
                line.clear();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                let value: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(value["id"], id);
            }
        });
        let mut output = Output::new(
            &json!({"events":{"max_pending_notification_bytes":4096,"backpressure_notice_ms":1000}}),
        );
        let session = Session {
            receive: 1048576,
            ..Session::default()
        };
        for id in 0..500 {
            output
                .send(
                    &mut server,
                    json!({"jsonrpc":"2.0","id":id,"result":"x".repeat(1000)}),
                    &session,
                )
                .await
                .unwrap();
            assert!(output.pending <= 4096);
        }
        assert!(
            output
                .drain_until(&mut server, Instant::now() + Duration::from_secs(1))
                .await
                .unwrap()
        );
        reader.await.unwrap();
    }
}
