use std::fs::{self, File};
use std::io::{self, BufWriter, Read as _, Seek as _, SeekFrom, Write as _};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
struct Chunk {
    offset: u64,
    length: usize,
}

#[derive(Debug)]
pub(crate) struct Spool {
    out: BufWriter<File>,
    chunks: Vec<Chunk>,
    end: u64,
}

impl Spool {
    pub(crate) fn new() -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| return elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!(".opcov-spool-{}-{stamp}", process::id()));
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        fs::remove_file(&path)?;
        return Ok(Self {
            out: BufWriter::new(file),
            chunks: Vec::new(),
            end: 0,
        });
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let length = u64::try_from(bytes.len()).map_err(io::Error::other)?;
        let end = self
            .end
            .checked_add(length)
            .ok_or_else(|| return io::Error::other("the spool exceeds 2^64 bytes"))?;
        self.out.write_all(bytes)?;
        let index = self.chunks.len();
        self.chunks.push(Chunk {
            offset: self.end,
            length: bytes.len(),
        });
        self.end = end;
        return Ok(index);
    }

    pub(crate) fn into_reader(self) -> io::Result<SpoolReader> {
        let file = self
            .out
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        return Ok(SpoolReader {
            file,
            chunks: self.chunks,
            buffer: Vec::new(),
        });
    }
}

#[derive(Debug)]
pub(crate) struct SpoolReader {
    file: File,
    chunks: Vec<Chunk>,
    buffer: Vec<u8>,
}

impl SpoolReader {
    pub(crate) fn chunk(&mut self, index: usize) -> io::Result<&[u8]> {
        let Some(chunk) = self.chunks.get(index).copied() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("spooled chunk {index} does not exist"),
            ));
        };
        self.buffer.resize(chunk.length, 0);
        self.file.seek(SeekFrom::Start(chunk.offset))?;
        self.file.read_exact(&mut self.buffer)?;
        return Ok(&self.buffer);
    }
}
