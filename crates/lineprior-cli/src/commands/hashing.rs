use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/// Reader that optionally hashes exactly the bytes observed by its consumer.
pub(crate) struct HashingReader<R> {
    inner: R,
    hasher: Option<Sha256>,
}

impl<R> HashingReader<R> {
    pub(crate) fn new(inner: R, enabled: bool) -> Self {
        Self {
            inner,
            hasher: enabled.then(Sha256::new),
        }
    }

    pub(crate) fn finish_sha256(self) -> Option<String> {
        self.hasher.map(|hasher| format!("{:x}", hasher.finalize()))
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if let Some(hasher) = &mut self.hasher {
            hasher.update(&buffer[..read]);
        }
        Ok(read)
    }
}

/// Writer that optionally hashes exactly the bytes accepted by its sink.
pub(crate) struct HashingWriter<W> {
    inner: W,
    hasher: Option<Sha256>,
}

impl<W> HashingWriter<W> {
    pub(crate) fn new(inner: W, enabled: bool) -> Self {
        Self {
            inner,
            hasher: enabled.then(Sha256::new),
        }
    }

    pub(crate) fn finish_sha256(self) -> Option<String> {
        self.hasher.map(|hasher| format!("{:x}", hasher.finalize()))
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        if let Some(hasher) = &mut self.hasher {
            hasher.update(&buffer[..written]);
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
