use std::collections::BTreeMap;
use std::io::Read;

use serde::Deserialize;

use super::error::{Collection, EntryError, ModelError, ReadError};
use super::model::{
    FILES, FileCoverage, GENERATOR, Header, LocalTests, PATHS, PathBase, Report, SCHEMA, SETTINGS,
    SourcePath, TESTS, TestRefs, check_schema, validate_entry,
};
use super::scan::{Found, Scanner};

#[derive(Debug)]
pub struct RawEntry {
    path: SourcePath,
    json: Vec<u8>,
}

impl RawEntry {
    #[must_use]
    pub const fn path(&self) -> &SourcePath {
        return &self.path;
    }

    pub fn decode(self, header: &Header) -> Result<(SourcePath, FileCoverage), ReadError> {
        return self.decode_with(&header.paths, &LocalTests::of(&header.tests));
    }

    pub(super) fn decode_with(
        self,
        paths: &PathBase,
        refs: &impl TestRefs,
    ) -> Result<(SourcePath, FileCoverage), ReadError> {
        let Self { path, json } = self;
        let decoded = serde_json::from_slice::<FileCoverage>(&json)
            .map_err(EntryError::Decode)
            .and_then(|file| {
                return validate_entry(paths, refs, &path, file)
                    .map_err(|error| return EntryError::Model(Box::new(error)));
            });
        drop(json);
        return match decoded {
            Ok(file) => Ok((path, file)),
            Err(error) => Err(ReadError::Entry {
                path,
                error: Box::new(error),
            }),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    First,
    Rest,
    Done,
}

#[derive(Debug)]
pub struct ReportReader<R> {
    scanner: Scanner<R>,
    header: Header,
    previous: Option<SourcePath>,
    state: State,
    scratch: Vec<u8>,
}

fn key<R: Read>(scanner: &mut Scanner<R>, scratch: &mut Vec<u8>) -> Result<String, ReadError> {
    scanner.string(scratch)?;
    let offset = scanner.offset();
    return serde_json::from_slice(scratch).map_err(|error| {
        return ReadError::Key { offset, error };
    });
}

fn member<R: Read, T: for<'de> Deserialize<'de>>(
    scanner: &mut Scanner<R>,
    scratch: &mut Vec<u8>,
    name: &'static str,
    first: bool,
) -> Result<T, ReadError> {
    let found = scanner.peek()?;
    if found == Found::Byte(b'}') {
        return Err(ReadError::Member {
            expected: name,
            found: None,
        });
    }
    if !first {
        scanner.expect(b',', "`,` between members")?;
    }
    let found = key(scanner, scratch)?;
    if found != name {
        return Err(ReadError::Member {
            expected: name,
            found: Some(found),
        });
    }
    scanner.expect(b':', "`:` after a member name")?;
    scanner.value(scratch)?;
    return serde_json::from_slice(scratch).map_err(|error| {
        return ReadError::Value {
            member: name,
            error,
        };
    });
}

impl<R: Read> ReportReader<R> {
    pub fn open(reader: R) -> Result<Self, ReadError> {
        let mut scanner = Scanner::new(reader);
        let mut scratch = Vec::new();
        scanner.expect(b'{', "`{` opening the report")?;
        let schema: String = member(&mut scanner, &mut scratch, SCHEMA, true)?;
        check_schema(&schema).map_err(ReadError::Schema)?;
        let generator = member(&mut scanner, &mut scratch, GENERATOR, false)?;
        let settings = member(&mut scanner, &mut scratch, SETTINGS, false)?;
        let paths = member(&mut scanner, &mut scratch, PATHS, false)?;
        let tests = member(&mut scanner, &mut scratch, TESTS, false)?;
        if scanner.peek()? == Found::Byte(b'}') {
            return Err(ReadError::Member {
                expected: FILES,
                found: None,
            });
        }
        scanner.expect(b',', "`,` between members")?;
        let found = key(&mut scanner, &mut scratch)?;
        if found != FILES {
            return Err(ReadError::Member {
                expected: FILES,
                found: Some(found),
            });
        }
        scanner.expect(b':', "`:` after a member name")?;
        scanner.expect(b'{', "`{` opening the files")?;
        return Ok(Self {
            scanner,
            header: Header {
                generator,
                settings,
                paths,
                tests,
            },
            previous: None,
            state: State::First,
            scratch,
        });
    }

    #[must_use]
    pub const fn header(&self) -> &Header {
        return &self.header;
    }

    #[must_use]
    pub fn into_header(self) -> Header {
        return self.header;
    }

    fn finish(&mut self) -> Result<(), ReadError> {
        self.scanner.expect(b'}', "`}` closing the files")?;
        let found = self.scanner.peek()?;
        if found != Found::Byte(b'}') {
            if found == Found::Byte(b',') {
                self.scanner.expect(b',', "`,`")?;
                let extra = key(&mut self.scanner, &mut self.scratch)?;
                return Err(ReadError::Trailing { member: extra });
            }
            return Err(self.scanner.unexpected("`}` closing the report", found));
        }
        self.scanner.expect(b'}', "`}` closing the report")?;
        return self.scanner.end();
    }

    fn entry(&mut self) -> Result<Option<RawEntry>, ReadError> {
        if self.state == State::Done {
            return Ok(None);
        }
        if self.scanner.peek()? == Found::Byte(b'}') {
            self.state = State::Done;
            self.finish()?;
            return Ok(None);
        }
        if self.state == State::Rest {
            self.scanner.expect(b',', "`,` between files")?;
        }
        let raw = key(&mut self.scanner, &mut self.scratch)?;
        let path =
            SourcePath::new(raw).map_err(|error| return ReadError::Files(Box::new(error)))?;
        if let Some(last) = &self.previous
            && *last >= path
        {
            return Err(ReadError::Files(Box::new(ModelError::NotAscending {
                collection: Collection::Files,
                previous: format!("{last:?}"),
                next: format!("{path:?}"),
            })));
        }
        self.header
            .paths
            .admits(&path)
            .map_err(|error| return ReadError::Files(Box::new(error)))?;
        self.scanner.expect(b':', "`:` after a file path")?;
        let mut json = Vec::new();
        self.scanner.value(&mut json)?;
        self.previous = Some(path.clone());
        self.state = State::Rest;
        return Ok(Some(RawEntry { path, json }));
    }
}

impl<R: Read> Iterator for ReportReader<R> {
    type Item = Result<RawEntry, ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.entry();
        if next.is_err() {
            self.state = State::Done;
        }
        return next.transpose();
    }
}

impl Report {
    pub fn read<R: Read>(reader: R) -> Result<Self, ReadError> {
        let mut entries = ReportReader::open(reader)?;
        let mut files = BTreeMap::new();
        while let Some(entry) = entries.next().transpose()? {
            let (path, file) = entry.decode(entries.header())?;
            files.insert(path, file);
        }
        return Ok(Self::from_validated(entries.into_header(), files));
    }
}
