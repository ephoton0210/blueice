// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private child-process pipes use the existing browser wire contract. A
//! worker assembles complete bounded messages before the session sees them,
//! so its polling timeout never loses a partially received pipe frame.

use super::ReadTimeout;
use std::cell::Cell;
use std::io::{self, Cursor, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

pub struct MessagePipe<W> {
    incoming: Receiver<io::Result<Vec<u8>>>,
    current: Cursor<Vec<u8>>,
    writer: W,
    timeout: Cell<Option<Duration>>,
}

impl<W> MessagePipe<W> {
    pub fn new<R: Read + Send + 'static>(mut reader: R, writer: W) -> Self {
        let (sender, incoming) = mpsc::sync_channel(4);
        std::thread::spawn(move || loop {
            let result = blueice_ipc::read_client_message_with_ids(&mut reader).and_then(
                |(tab, request, message)| {
                    let mut bytes = Vec::new();
                    blueice_ipc::write_client_message_with_ids(&mut bytes, tab, request, &message)?;
                    Ok(bytes)
                },
            );
            let failed = result.is_err();
            if sender.send(result).is_err() || failed {
                break;
            }
        });
        Self {
            incoming,
            current: Cursor::new(Vec::new()),
            writer,
            timeout: Cell::new(None),
        }
    }
}

impl<W> ReadTimeout for MessagePipe<W> {
    fn set_read_timeout(&self, duration: Option<Duration>) -> io::Result<()> {
        self.timeout.set(duration);
        Ok(())
    }
}

impl<W> Read for MessagePipe<W> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.current.position() == self.current.get_ref().len() as u64 {
            let result = match self.timeout.get() {
                Some(timeout) => match self.incoming.recv_timeout(timeout) {
                    Ok(result) => result,
                    Err(RecvTimeoutError::Timeout) => return Err(io::ErrorKind::TimedOut.into()),
                    Err(RecvTimeoutError::Disconnected) => return Ok(0),
                },
                None => match self.incoming.recv() {
                    Ok(result) => result,
                    Err(_) => return Ok(0),
                },
            };
            self.current = Cursor::new(result?);
        }
        self.current.read(buffer)
    }
}

impl<W: Write> Write for MessagePipe<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.writer.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
