use crate::{Error, MAX_BATCH_ELEMENTS, MAX_FRAME_BYTES, READ_SCRATCH_BYTES};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};

pub(crate) struct FrameReader<R> {
    reader: R,
    scratch: [u8; READ_SCRATCH_BYTES],
    cursor: usize,
    available: usize,
    line: Vec<u8>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader,
            scratch: [0; READ_SCRATCH_BYTES],
            cursor: 0,
            available: 0,
            line: Vec::with_capacity(READ_SCRATCH_BYTES),
        }
    }

    pub(crate) async fn next(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            if self.cursor == self.available {
                self.available = self
                    .reader
                    .read(&mut self.scratch)
                    .await
                    .map_err(|_| Error::TransportFailure)?;
                self.cursor = 0;
                if self.available == 0 {
                    return if self.line.is_empty() {
                        Ok(None)
                    } else {
                        Err(Error::TruncatedFrame)
                    };
                }
            }
            while self.cursor < self.available {
                let byte = self.scratch[self.cursor];
                self.cursor += 1;
                if byte == b'\n' {
                    return Ok(Some(std::mem::take(&mut self.line)));
                }
                if self.line.len() == MAX_FRAME_BYTES {
                    return Err(Error::FrameTooLarge);
                }
                self.line.push(byte);
            }
        }
    }
}

pub(crate) fn parse_json_line(bytes: &[u8]) -> Result<Value, Error> {
    let input = std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;
    serde_json::from_str(input).map_err(|_| Error::MalformedJson)
}

pub(crate) fn validate_frame_shape(value: Value) -> Result<(Vec<Value>, bool), Error> {
    match value {
        Value::Array(values) => {
            if values.is_empty() {
                return Err(Error::EmptyBatch);
            }
            if values.len() > MAX_BATCH_ELEMENTS {
                return Err(Error::BatchTooLarge);
            }
            Ok((values, true))
        }
        value => Ok((vec![value], false)),
    }
}
