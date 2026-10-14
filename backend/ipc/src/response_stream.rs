// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded framing for one original-response body on a dedicated connection.
//! A transport disconnect is never a successful unknown-length end-of-body.

use std::io::{self, Read, Write};

pub const MAX_RESPONSE_CHUNK_BYTES: usize = 64 * 1024;

pub fn write_response_chunk(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_RESPONSE_CHUNK_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "oversized response chunk",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(bytes)
}

pub struct ResponseBodyReader<R> {
    input: R,
    header: [u8; 4],
    header_used: usize,
    remaining: usize,
    finished: bool,
}

impl<R> ResponseBodyReader<R> {
    pub fn new(input: R) -> Self {
        Self {
            input,
            header: [0; 4],
            header_used: 0,
            remaining: 0,
            finished: false,
        }
    }
}

impl<R: Read> Read for ResponseBodyReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.finished {
            return Ok(0);
        }
        if self.remaining == 0 {
            while self.header_used < 4 {
                let count = self.input.read(&mut self.header[self.header_used..])?;
                if count == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "response ended without a completion marker",
                    ));
                }
                self.header_used += count;
            }
            self.remaining = u32::from_be_bytes(self.header) as usize;
            self.header_used = 0;
            if self.remaining > MAX_RESPONSE_CHUNK_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized response chunk",
                ));
            }
            if self.remaining == 0 {
                self.finished = true;
                return Ok(0);
            }
        }
        let limit = buffer.len().min(self.remaining);
        let count = self.input.read(&mut buffer[..limit])?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated response chunk",
            ));
        }
        self.remaining -= count;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_chunks_require_an_explicit_completion_marker() {
        let mut wire = Vec::new();
        write_response_chunk(&mut wire, &[0xff, 0, 0x80]).unwrap();
        write_response_chunk(&mut wire, b"original").unwrap();
        write_response_chunk(&mut wire, &[]).unwrap();
        let mut bytes = Vec::new();
        ResponseBodyReader::new(wire.as_slice())
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            bytes,
            [0xff, 0, 0x80, b'o', b'r', b'i', b'g', b'i', b'n', b'a', b'l']
        );
    }

    #[test]
    fn disconnects_and_oversized_chunks_fail() {
        for wire in [vec![], vec![0, 0], vec![0, 0, 0, 3, 1], vec![0, 1, 0, 1]] {
            assert!(ResponseBodyReader::new(wire.as_slice())
                .read_to_end(&mut Vec::new())
                .is_err());
        }
        assert!(
            write_response_chunk(&mut Vec::new(), &vec![0; MAX_RESPONSE_CHUNK_BYTES + 1]).is_err()
        );
    }

    #[test]
    fn read_timeouts_preserve_a_partly_received_header() {
        struct Input {
            calls: usize,
        }
        impl Read for Input {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                let bytes: &[u8] = match self.calls {
                    1 => &[0, 0],
                    2 => return Err(io::ErrorKind::TimedOut.into()),
                    3 => &[0, 1],
                    4 => &[0xff],
                    _ => &[0, 0, 0, 0],
                };
                out[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
        }
        let mut reader = ResponseBodyReader::new(Input { calls: 0 });
        let mut out = [0; 4];
        assert_eq!(
            reader.read(&mut out).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(reader.read(&mut out).unwrap(), 1);
        assert_eq!(out[0], 0xff);
        assert_eq!(reader.read(&mut out).unwrap(), 0);
    }
}
