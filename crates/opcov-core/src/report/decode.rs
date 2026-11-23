use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

use serde::de::{self, DeserializeSeed, MapAccess, Visitor};
use serde::{Deserialize, Deserializer as _};

use super::error::{Collection, ModelError, ReadError, SchemaError};
use super::model::{
    FILES, FileCoverage, GENERATOR, Header, LocalTests, PATHS, PathBase, Report, SCHEMA, SETTINGS,
    SourcePath, TESTS, TestRefs, check_schema, validate_entry,
};

#[derive(Debug)]
pub(super) struct Abandoned;

impl fmt::Display for Abandoned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str("reading was abandoned by the consumer");
    }
}

pub(super) trait FileSink {
    fn begin(&mut self, path: &SourcePath) -> Result<(), Abandoned>;
    fn accept(&mut self, path: SourcePath, file: FileCoverage) -> Result<(), Abandoned>;
}

pub(super) trait Document {
    type Output;
    fn visit<'de, A: MapAccess<'de>>(self, map: &mut A) -> Result<Self::Output, A::Error>;
}

pub(super) fn expect_member<'de, A: MapAccess<'de>>(
    map: &mut A,
    name: &'static str,
) -> Result<(), A::Error> {
    return match map.next_key::<String>()? {
        Some(found) if found == name => Ok(()),
        Some(found) => Err(de::Error::custom(format!(
            "expected member `{name}`, found `{found}`"
        ))),
        None => Err(de::Error::missing_field(name)),
    };
}

fn member<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    name: &'static str,
) -> Result<T, A::Error> {
    expect_member(map, name)?;
    return map.next_value();
}

pub(super) fn header<'de, A: MapAccess<'de>>(map: &mut A) -> Result<Header, A::Error> {
    let generator = member(map, GENERATOR)?;
    let settings = member(map, SETTINGS)?;
    let paths = member(map, PATHS)?;
    let tests = member(map, TESTS)?;
    return Ok(Header {
        generator,
        settings,
        paths,
        tests,
    });
}

pub(super) struct FilesSeed<'a, R, S> {
    pub(super) paths: &'a PathBase,
    pub(super) refs: &'a R,
    pub(super) sink: &'a mut S,
}

impl<'de, R: TestRefs, S: FileSink> DeserializeSeed<'de> for FilesSeed<'_, R, S> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        return deserializer.deserialize_map(self);
    }
}

impl<'de, R: TestRefs, S: FileSink> Visitor<'de> for FilesSeed<'_, R, S> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str("an object of files in ascending path order");
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut previous: Option<SourcePath> = None;
        while let Some(raw) = map.next_key::<String>()? {
            let path = SourcePath::new(raw).map_err(de::Error::custom)?;
            if let Some(last) = &previous
                && *last >= path
            {
                return Err(de::Error::custom(ModelError::NotAscending {
                    collection: Collection::Files,
                    previous: format!("{last:?}"),
                    next: format!("{path:?}"),
                }));
            }
            self.paths.admits(&path).map_err(de::Error::custom)?;
            self.sink.begin(&path).map_err(de::Error::custom)?;
            let file = map.next_value::<FileCoverage>()?;
            let file =
                validate_entry(self.paths, self.refs, &path, file).map_err(de::Error::custom)?;
            previous = Some(path.clone());
            self.sink.accept(path, file).map_err(de::Error::custom)?;
        }
        return Ok(());
    }
}

struct Envelope<'s, D> {
    document: D,
    schema_error: &'s mut Option<SchemaError>,
}

impl<'de, D: Document> Visitor<'de> for Envelope<'_, D> {
    type Value = D::Output;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str("an opcov report object");
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<D::Output, A::Error> {
        let found: String = member(&mut map, SCHEMA)?;
        if let Err(error) = check_schema(&found) {
            let message = error.to_string();
            *self.schema_error = Some(error);
            return Err(de::Error::custom(message));
        }
        let output = self.document.visit(&mut map)?;
        if let Some(extra) = map.next_key::<String>()? {
            return Err(de::Error::custom(format!(
                "unexpected member `{extra}` after `{FILES}`"
            )));
        }
        return Ok(output);
    }
}

pub(super) fn read_document<R: Read, D: Document>(
    reader: R,
    document: D,
) -> Result<D::Output, ReadError> {
    let mut schema_error = None;
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let result = deserializer
        .deserialize_map(Envelope {
            document,
            schema_error: &mut schema_error,
        })
        .and_then(|output| {
            deserializer.end()?;
            return Ok(output);
        });
    return result.map_err(|error| {
        return schema_error.map_or_else(|| return ReadError::Decode(error), ReadError::Schema);
    });
}

struct Collect(BTreeMap<SourcePath, FileCoverage>);

impl FileSink for Collect {
    fn begin(&mut self, _path: &SourcePath) -> Result<(), Abandoned> {
        return Ok(());
    }

    fn accept(&mut self, path: SourcePath, file: FileCoverage) -> Result<(), Abandoned> {
        self.0.insert(path, file);
        return Ok(());
    }
}

struct FullDocument;

impl Document for FullDocument {
    type Output = Report;

    fn visit<'de, A: MapAccess<'de>>(self, map: &mut A) -> Result<Report, A::Error> {
        let header = header(map)?;
        expect_member(map, FILES)?;
        let mut files = Collect(BTreeMap::new());
        map.next_value_seed(FilesSeed {
            paths: &header.paths,
            refs: &LocalTests::of(&header.tests),
            sink: &mut files,
        })?;
        return Ok(Report::from_validated(header, files.0));
    }
}

impl Report {
    pub fn read<R: Read>(reader: R) -> Result<Self, ReadError> {
        return read_document(reader, FullDocument);
    }
}
