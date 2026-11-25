use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvError, SyncSender};
use std::thread;

use super::combine::{EntrySource, InputId, MergeError, MergePlan, Remap, merge_entries, plan};
use super::decode::ReportReader;
use super::encode::{ReportWriter, WriteError};
use super::error::{ReadError, SchemaError};
use super::model::{FileCoverage, Header, PathBase, SourcePath};

#[derive(Debug)]
pub enum StreamMergeError {
    Open { input: InputId, error: io::Error },
    Read { input: InputId, error: ReadError },
    Merge(MergeError),
    ReaderStopped { input: InputId },
    Write(WriteError),
}

impl StreamMergeError {
    #[must_use]
    pub const fn is_refusal(&self) -> bool {
        return match self {
            Self::Merge(error) => error.is_refusal(),
            Self::Read {
                error: ReadError::Schema(SchemaError::Unsupported { .. }),
                ..
            } => true,
            Self::Open { .. } | Self::Read { .. } | Self::ReaderStopped { .. } | Self::Write(_) => {
                false
            }
        };
    }

    #[must_use]
    pub fn describe<'a, N: fmt::Display>(&'a self, names: &'a [N]) -> impl fmt::Display + 'a {
        return Described { error: self, names };
    }

    fn render(
        &self,
        formatter: &mut fmt::Formatter<'_>,
        name: &dyn Fn(InputId) -> String,
    ) -> fmt::Result {
        return match self {
            Self::Open { input, error } => {
                write!(formatter, "{}: cannot open: {error}", name(*input))
            }
            Self::Read { input, error } => write!(formatter, "{}: {error}", name(*input)),
            Self::Merge(error) => error.render(formatter, name),
            Self::ReaderStopped { input } => {
                write!(
                    formatter,
                    "{}: the reader stopped unexpectedly",
                    name(*input)
                )
            }
            Self::Write(error) => write!(formatter, "cannot write the merged report: {error}"),
        };
    }
}

struct Described<'a, N> {
    error: &'a StreamMergeError,
    names: &'a [N],
}

impl<N: fmt::Display> fmt::Display for Described<'_, N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return self.error.render(formatter, &|input| {
            return self
                .names
                .get(input.get())
                .map_or_else(|| return input.to_string(), ToString::to_string);
        });
    }
}

impl fmt::Display for StreamMergeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return self.render(formatter, &|input| return input.to_string());
    }
}

impl Error for StreamMergeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return match self {
            Self::Open { error, .. } => Some(error),
            Self::Write(error) => Some(error),
            Self::Read { error, .. } => Some(error),
            Self::Merge(error) => Some(error),
            Self::ReaderStopped { .. } => None,
        };
    }
}

impl From<MergeError> for StreamMergeError {
    fn from(error: MergeError) -> Self {
        return Self::Merge(error);
    }
}

const PREFETCH: usize = 4;

enum Prefetched {
    Entry(SourcePath, FileCoverage),
    End,
    Failed(ReadError),
}

fn prefetch<R: Read>(
    mut reader: ReportReader<R>,
    paths: &PathBase,
    remap: &Remap,
    entries: &SyncSender<Prefetched>,
) {
    loop {
        let message = match reader.next() {
            None => Prefetched::End,
            Some(Err(error)) => Prefetched::Failed(error),
            Some(Ok(entry)) => match entry.decode_with(paths, remap) {
                Ok((path, file)) => Prefetched::Entry(path, file),
                Err(error) => Prefetched::Failed(error),
            },
        };
        let last = !matches!(message, Prefetched::Entry(..));
        if entries.send(message).is_err() || last {
            return;
        }
    }
}

pub(super) struct StreamSource {
    input: InputId,
    entries: Receiver<Prefetched>,
    head: Option<(SourcePath, FileCoverage)>,
}

impl StreamSource {
    pub(super) fn start<'s, 'p: 's, R: Read + Send + 's>(
        scope: &'s thread::Scope<'s, 'p>,
        input: InputId,
        reader: ReportReader<R>,
        paths: &'p PathBase,
        remap: &'p Remap,
    ) -> Result<Self, StreamMergeError> {
        let (sender, entries) = mpsc::sync_channel(PREFETCH);
        scope.spawn(move || prefetch(reader, paths, remap, &sender));
        let mut source = Self {
            input,
            entries,
            head: None,
        };
        source.advance()?;
        return Ok(source);
    }

    fn advance(&mut self) -> Result<(), StreamMergeError> {
        let input = self.input;
        self.head = match self.entries.recv() {
            Ok(Prefetched::Entry(path, file)) => Some((path, file)),
            Ok(Prefetched::End) => None,
            Ok(Prefetched::Failed(error)) => return Err(StreamMergeError::Read { input, error }),
            Err(RecvError) => return Err(StreamMergeError::ReaderStopped { input }),
        };
        return Ok(());
    }
}

impl EntrySource for StreamSource {
    type Error = StreamMergeError;

    fn input(&self) -> InputId {
        return self.input;
    }

    fn head(&self) -> Option<&SourcePath> {
        return self.head.as_ref().map(|(path, _)| return path);
    }

    fn take(&mut self) -> Result<Option<FileCoverage>, StreamMergeError> {
        let Some((_, file)) = self.head.take() else {
            return Ok(None);
        };
        self.advance()?;
        return Ok(Some(file));
    }
}

pub fn merge_streams<S, R, W>(
    inputs: &[S],
    open: impl Fn(&S) -> io::Result<R>,
    out: W,
) -> Result<W, StreamMergeError>
where
    R: Read + Send,
    W: Write,
{
    let readers = inputs
        .iter()
        .enumerate()
        .map(|(position, source)| {
            let input = InputId::new(position);
            let reader =
                open(source).map_err(|error| return StreamMergeError::Open { input, error })?;
            return ReportReader::open(reader)
                .map_err(|error| return StreamMergeError::Read { input, error });
        })
        .collect::<Result<Vec<_>, StreamMergeError>>()?;
    let MergePlan { header, remaps } = {
        let headers: Vec<&Header> = readers.iter().map(ReportReader::header).collect();
        plan(&headers)?
    };
    let mut writer = ReportWriter::begin(out, &header)
        .map_err(|error| return StreamMergeError::Write(WriteError::Io(error)))?;
    let paths = &header.paths;
    thread::scope(|scope| {
        let mut sources = Vec::with_capacity(readers.len());
        for ((position, reader), remap) in readers.into_iter().enumerate().zip(&remaps) {
            sources.push(StreamSource::start(
                scope,
                InputId::new(position),
                reader,
                paths,
                remap,
            )?);
        }
        return merge_entries(&mut sources, |path, file| {
            return writer.entry(&path, &file).map_err(StreamMergeError::Write);
        });
    })?;
    return writer
        .finish()
        .map_err(|error| return StreamMergeError::Write(WriteError::Io(error)));
}
