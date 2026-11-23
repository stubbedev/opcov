use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvError, SendError, SyncSender};
use std::thread;

use serde::de::{IgnoredAny, MapAccess};

use super::combine::{EntrySource, InputId, MergeError, MergePlan, Remap, merge_entries, plan};
use super::decode::{
    Abandoned, Document, FileSink, FilesSeed, expect_member, header, read_document,
};
use super::encode::DocumentWriter;
use super::error::{ReadError, SchemaError};
use super::model::{
    FILES, FileCoverage, GENERATOR, Header, PATHS, PathBase, SETTINGS, SourcePath, TESTS, TestRefs,
};

const HEADER_MEMBERS: [&str; 4] = [GENERATOR, SETTINGS, PATHS, TESTS];

#[derive(Debug)]
pub enum StreamMergeError {
    Open { input: InputId, error: io::Error },
    Read { input: InputId, error: ReadError },
    Merge(MergeError),
    ReaderStopped { input: InputId },
    Write(io::Error),
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
            Self::Open { error, .. } | Self::Write(error) => Some(error),
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

impl<T> From<SendError<T>> for Abandoned {
    fn from(_closed: SendError<T>) -> Self {
        return Self;
    }
}

impl From<RecvError> for Abandoned {
    fn from(_closed: RecvError) -> Self {
        return Self;
    }
}

fn skip_member<'de, A: MapAccess<'de>>(map: &mut A, name: &'static str) -> Result<(), A::Error> {
    expect_member(map, name)?;
    map.next_value::<IgnoredAny>()?;
    return Ok(());
}

struct HeaderDocument;

impl Document for HeaderDocument {
    type Output = Header;

    fn visit<'de, A: MapAccess<'de>>(self, map: &mut A) -> Result<Header, A::Error> {
        let header = header(map)?;
        skip_member(map, FILES)?;
        return Ok(header);
    }
}

struct FilesDocument<'a, R, S> {
    paths: &'a PathBase,
    refs: &'a R,
    sink: &'a mut S,
}

impl<R: TestRefs, S: FileSink> Document for FilesDocument<'_, R, S> {
    type Output = ();

    fn visit<'de, A: MapAccess<'de>>(self, map: &mut A) -> Result<(), A::Error> {
        for name in HEADER_MEMBERS {
            skip_member(map, name)?;
        }
        expect_member(map, FILES)?;
        return map.next_value_seed(FilesSeed {
            paths: self.paths,
            refs: self.refs,
            sink: self.sink,
        });
    }
}

enum KeyMessage {
    Key(SourcePath),
    End,
    Failed(StreamMergeError),
}

enum ValueMessage {
    Value(FileCoverage),
    Failed(StreamMergeError),
}

struct ChannelSink {
    keys: SyncSender<KeyMessage>,
    values: SyncSender<ValueMessage>,
    go: Receiver<()>,
    awaiting_value: bool,
}

impl FileSink for ChannelSink {
    fn begin(&mut self, path: &SourcePath) -> Result<(), Abandoned> {
        self.keys.send(KeyMessage::Key(path.clone()))?;
        self.go.recv()?;
        self.awaiting_value = true;
        return Ok(());
    }

    fn accept(&mut self, _path: SourcePath, file: FileCoverage) -> Result<(), Abandoned> {
        self.values.send(ValueMessage::Value(file))?;
        self.awaiting_value = false;
        return Ok(());
    }
}

fn feed<R: Read>(
    input: InputId,
    reader: io::Result<R>,
    paths: &PathBase,
    remap: &Remap,
    mut sink: ChannelSink,
) -> Result<(), Abandoned> {
    let outcome = match reader {
        Err(error) => Err(StreamMergeError::Open { input, error }),
        Ok(reader) => read_document(
            reader,
            FilesDocument {
                paths,
                refs: remap,
                sink: &mut sink,
            },
        )
        .map_err(|error| return StreamMergeError::Read { input, error }),
    };
    let ChannelSink {
        keys,
        values,
        awaiting_value,
        ..
    } = sink;
    return match outcome {
        Ok(()) => keys.send(KeyMessage::End).map_err(Abandoned::from),
        Err(error) if awaiting_value => values
            .send(ValueMessage::Failed(error))
            .map_err(Abandoned::from),
        Err(error) => keys
            .send(KeyMessage::Failed(error))
            .map_err(Abandoned::from),
    };
}

struct StreamSource {
    input: InputId,
    keys: Receiver<KeyMessage>,
    values: Receiver<ValueMessage>,
    go: SyncSender<()>,
    head: Option<SourcePath>,
}

impl StreamSource {
    fn advance(&mut self) -> Result<(), StreamMergeError> {
        self.head = match self.keys.recv() {
            Ok(KeyMessage::Key(path)) => Some(path),
            Ok(KeyMessage::End) => None,
            Ok(KeyMessage::Failed(error)) => return Err(error),
            Err(RecvError) => return Err(self.stopped()),
        };
        return Ok(());
    }

    const fn stopped(&self) -> StreamMergeError {
        return StreamMergeError::ReaderStopped { input: self.input };
    }
}

impl EntrySource for StreamSource {
    type Error = StreamMergeError;

    fn input(&self) -> InputId {
        return self.input;
    }

    fn head(&self) -> Option<&SourcePath> {
        return self.head.as_ref();
    }

    fn take(&mut self) -> Result<Option<FileCoverage>, StreamMergeError> {
        if self.head.is_none() {
            return Ok(None);
        }
        if self.go.send(()).is_err() {
            return Err(self.stopped());
        }
        let file = match self.values.recv() {
            Ok(ValueMessage::Value(file)) => file,
            Ok(ValueMessage::Failed(error)) => return Err(error),
            Err(RecvError) => return Err(self.stopped()),
        };
        self.advance()?;
        return Ok(Some(file));
    }
}

fn read_header<R: Read>(input: InputId, reader: io::Result<R>) -> Result<Header, StreamMergeError> {
    let reader = reader.map_err(|error| return StreamMergeError::Open { input, error })?;
    return read_document(reader, HeaderDocument)
        .map_err(|error| return StreamMergeError::Read { input, error });
}

fn read_headers<S, R>(
    inputs: &[S],
    open: &(impl Fn(&S) -> io::Result<R> + Sync),
) -> Result<Vec<Header>, StreamMergeError>
where
    S: Sync,
    R: Read,
{
    return thread::scope(|scope| {
        let readers: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(position, source)| {
                let input = InputId::new(position);
                return (
                    input,
                    scope.spawn(move || return read_header(input, open(source))),
                );
            })
            .collect();
        return readers
            .into_iter()
            .map(|(input, reader)| {
                return reader
                    .join()
                    .map_err(|_panic| return StreamMergeError::ReaderStopped { input })?;
            })
            .collect();
    });
}

pub fn merge_streams<S, R, W>(
    inputs: &[S],
    open: impl Fn(&S) -> io::Result<R> + Sync,
    out: W,
) -> Result<W, StreamMergeError>
where
    S: Sync,
    R: Read,
    W: Write,
{
    let MergePlan { header, remaps } = {
        let headers = read_headers(inputs, &open)?;
        let borrowed: Vec<&Header> = headers.iter().collect();
        plan(&borrowed)?
    };
    let mut writer = DocumentWriter::begin(out, &header).map_err(StreamMergeError::Write)?;
    thread::scope(|scope| {
        let mut sources = Vec::with_capacity(inputs.len());
        for ((position, source), remap) in inputs.iter().enumerate().zip(&remaps) {
            let input = InputId::new(position);
            let (key_sender, keys) = mpsc::sync_channel(1);
            let (value_sender, values) = mpsc::sync_channel(1);
            let (go, go_receiver) = mpsc::sync_channel(1);
            let sink = ChannelSink {
                keys: key_sender,
                values: value_sender,
                go: go_receiver,
                awaiting_value: false,
            };
            let paths = &header.paths;
            let open = &open;
            scope.spawn(move || return feed(input, open(source), paths, remap, sink));
            sources.push(StreamSource {
                input,
                keys,
                values,
                go,
                head: None,
            });
        }
        for source in &mut sources {
            source.advance()?;
        }
        return merge_entries(&mut sources, |path, file| {
            return writer.entry(&path, &file).map_err(StreamMergeError::Write);
        });
    })?;
    return writer.finish().map_err(StreamMergeError::Write);
}
