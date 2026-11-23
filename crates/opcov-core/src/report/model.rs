use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256 as Hasher};

use super::error::{Collection, HitSite, ModelError, SchemaError};
use crate::cfg::{FunctionKey, Line, LoopShape, OpIndex};
use crate::kind::EdgeKind;

const SCHEMA_FAMILY: &str = "opcov-report";
const SCHEMA_MAJOR: u32 = 1;

pub(super) const SCHEMA: &str = "schema";
pub(super) const GENERATOR: &str = "generator";
pub(super) const SETTINGS: &str = "settings";
pub(super) const PATHS: &str = "paths";
pub(super) const TESTS: &str = "tests";
pub(super) const FILES: &str = "files";

#[must_use]
pub fn schema_id() -> String {
    return format!("{SCHEMA_FAMILY}/{SCHEMA_MAJOR}");
}

pub(super) fn check_schema(found: &str) -> Result<(), SchemaError> {
    let major = found
        .strip_prefix(SCHEMA_FAMILY)
        .and_then(|rest| return rest.strip_prefix('/'))
        .and_then(parse_canonical_u32);
    return match major {
        Some(SCHEMA_MAJOR) => Ok(()),
        Some(_) => Err(SchemaError::Unsupported {
            found: found.to_owned(),
        }),
        None => Err(SchemaError::Invalid {
            found: found.to_owned(),
        }),
    };
}

fn parse_canonical_u32(raw: &str) -> Option<u32> {
    let digits = !raw.is_empty() && raw.bytes().all(|byte| return byte.is_ascii_digit());
    if !digits || (raw.len() > 1 && raw.starts_with('0')) {
        return None;
    }
    return raw.parse().ok();
}

pub(super) fn ensure_ascending<T>(
    items: &[T],
    collection: Collection,
    ordered: impl Fn(&T, &T) -> bool,
    render: impl Fn(&T) -> String,
) -> Result<(), ModelError> {
    for pair in items.windows(2) {
        if let [previous, next] = pair
            && !ordered(previous, next)
        {
            return Err(ModelError::NotAscending {
                collection,
                previous: render(previous),
                next: render(next),
            });
        }
    }
    return Ok(());
}

fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    return Option::<T>::deserialize(deserializer);
}

fn ascending_set<'de, D>(deserializer: D) -> Result<BTreeSet<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let items = Vec::<String>::deserialize(deserializer)?;
    ensure_ascending(
        &items,
        Collection::PathList,
        |previous, next| return previous < next,
        |item| return format!("{item:?}"),
    )
    .map_err(de::Error::custom)?;
    return Ok(items.into_iter().collect());
}

trait MapKey: Ord + fmt::Display + Sized {
    const COLLECTION: Collection;
    const ASCENDING: bool;
    fn parse_key(raw: String) -> Result<Self, ModelError>;
}

struct AscendingMap<K, V>(PhantomData<(K, V)>);

impl<'de, K: MapKey, V: Deserialize<'de>> Visitor<'de> for AscendingMap<K, V> {
    type Value = BTreeMap<K, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(formatter, "an object of {} with unique keys", K::COLLECTION);
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut entries: BTreeMap<K, V> = BTreeMap::new();
        while let Some(raw) = map.next_key::<String>()? {
            let key = K::parse_key(raw).map_err(de::Error::custom)?;
            if K::ASCENDING
                && let Some((last, _)) = entries.last_key_value()
                && *last >= key
            {
                return Err(de::Error::custom(ModelError::NotAscending {
                    collection: K::COLLECTION,
                    previous: last.to_string(),
                    next: key.to_string(),
                }));
            }
            if entries.contains_key(&key) {
                return Err(de::Error::custom(ModelError::DuplicateKey {
                    collection: K::COLLECTION,
                    key: key.to_string(),
                }));
            }
            let value = map.next_value::<V>()?;
            entries.insert(key, value);
        }
        return Ok(entries);
    }
}

fn ascending_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: MapKey,
    V: Deserialize<'de>,
{
    return deserializer.deserialize_map(AscendingMap(PhantomData));
}

impl MapKey for Line {
    const COLLECTION: Collection = Collection::Lines;
    const ASCENDING: bool = true;

    fn parse_key(raw: String) -> Result<Self, ModelError> {
        return parse_canonical_u32(&raw)
            .and_then(Self::new)
            .ok_or_else(|| return ModelError::InvalidLineKey { key: raw });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Column(NonZeroU32);

impl Column {
    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0.get();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "[u32; 4]", into = "[u32; 4]")]
pub struct Span {
    start: (Line, Column),
    end: (Line, Column),
}

impl Span {
    pub fn new(raw: [u32; 4]) -> Result<Self, ModelError> {
        let [line_start, col_start, line_end, col_end] = raw;
        let (Some(line_start), Some(col_start), Some(line_end), Some(col_end)) = (
            Line::new(line_start),
            NonZeroU32::new(col_start),
            Line::new(line_end),
            NonZeroU32::new(col_end),
        ) else {
            return Err(ModelError::InvalidSpan { span: raw });
        };
        let start = (line_start, Column(col_start));
        let end = (line_end, Column(col_end));
        if start >= end {
            return Err(ModelError::InvalidSpan { span: raw });
        }
        return Ok(Self { start, end });
    }

    #[must_use]
    pub const fn start(self) -> (Line, Column) {
        return self.start;
    }

    #[must_use]
    pub const fn end(self) -> (Line, Column) {
        return self.end;
    }

    #[must_use]
    pub fn contains_line(self, line: Line) -> bool {
        return self.start.0 <= line && line <= self.end.0;
    }
}

impl TryFrom<[u32; 4]> for Span {
    type Error = ModelError;

    fn try_from(raw: [u32; 4]) -> Result<Self, ModelError> {
        return Self::new(raw);
    }
}

impl From<Span> for [u32; 4] {
    fn from(span: Span) -> Self {
        return [
            span.start.0.get(),
            span.start.1.get(),
            span.end.0.get(),
            span.end.1.get(),
        ];
    }
}

impl fmt::Display for Span {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "{}:{}-{}:{}",
            self.start.0,
            self.start.1.get(),
            self.end.0,
            self.end.1.get()
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256([u8; 32]);

impl Sha256 {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        return Self(bytes);
    }

    #[must_use]
    pub fn digest(content: &[u8]) -> Self {
        return Self(Hasher::digest(content).into());
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        return &self.0;
    }

    pub fn parse(hex: &str) -> Result<Self, ModelError> {
        let invalid = || {
            return ModelError::InvalidSha256 {
                value: hex.to_owned(),
            };
        };
        let lowercase_hex = hex
            .bytes()
            .all(|byte| return matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        if !lowercase_hex || hex.len() != 64 {
            return Err(invalid());
        }
        let mut bytes = [0_u8; 32];
        let (pairs, _rest) = hex.as_bytes().as_chunks::<2>();
        for (slot, pair) in bytes.iter_mut().zip(pairs) {
            let digits = std::str::from_utf8(pair).map_err(|_utf8| return invalid())?;
            *slot = u8::from_str_radix(digits, 16).map_err(|_digits| return invalid())?;
        }
        return Ok(Self(bytes));
    }
}

impl TryFrom<String> for Sha256 {
    type Error = ModelError;

    fn try_from(hex: String) -> Result<Self, ModelError> {
        return Self::parse(&hex);
    }
}

impl From<Sha256> for String {
    fn from(sha: Sha256) -> Self {
        return sha.to_string();
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        return Ok(());
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(formatter, "Sha256({self})");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TestIndex(u32);

impl TestIndex {
    #[must_use]
    pub const fn new(index: u32) -> Self {
        return Self(index);
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0;
    }

    pub(super) fn position(self) -> Option<usize> {
        return usize::try_from(self.0).ok();
    }
}

impl fmt::Display for TestIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(formatter, "{}", self.0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>")]
pub struct TestTable(pub(super) Vec<String>);

impl TestTable {
    pub fn new(names: Vec<String>) -> Result<Self, ModelError> {
        if names.iter().any(String::is_empty) {
            return Err(ModelError::EmptyTestName);
        }
        if u32::try_from(names.len()).is_err() {
            return Err(ModelError::TooManyTests { count: names.len() });
        }
        ensure_ascending(
            &names,
            Collection::Tests,
            |previous, next| return previous < next,
            |name| return format!("{name:?}"),
        )?;
        return Ok(Self(names));
    }

    pub fn from_unordered(names: &[String]) -> Result<(Self, Vec<TestIndex>), ModelError> {
        let mut order: Vec<usize> = (0..names.len()).collect();
        order.sort_unstable_by(|left, right| return names.get(*left).cmp(&names.get(*right)));
        let mut indexes = vec![TestIndex(0); names.len()];
        let mut sorted = Vec::with_capacity(names.len());
        for position in order {
            let (Some(name), Some(slot), Ok(index)) = (
                names.get(position),
                indexes.get_mut(position),
                u32::try_from(sorted.len()),
            ) else {
                return Err(ModelError::TooManyTests { count: names.len() });
            };
            *slot = TestIndex(index);
            sorted.push(name.clone());
        }
        return Ok((Self::new(sorted)?, indexes));
    }

    #[must_use]
    pub fn names(&self) -> &[String] {
        return &self.0;
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        return self.0.len();
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        return self.0.is_empty();
    }

    #[must_use]
    pub fn name(&self, index: TestIndex) -> Option<&str> {
        return self.0.get(index.position()?).map(String::as_str);
    }
}

impl TryFrom<Vec<String>> for TestTable {
    type Error = ModelError;

    fn try_from(names: Vec<String>) -> Result<Self, ModelError> {
        return Self::new(names);
    }
}

pub(super) trait TestRefs {
    fn resolve(&self, index: TestIndex) -> Option<TestIndex>;
    fn count(&self) -> usize;
}

pub(super) struct LocalTests(usize);

impl LocalTests {
    pub(super) const fn of(tests: &TestTable) -> Self {
        return Self(tests.len());
    }
}

impl TestRefs for LocalTests {
    fn resolve(&self, index: TestIndex) -> Option<TestIndex> {
        return (index.position()? < self.0).then_some(index);
    }

    fn count(&self) -> usize {
        return self.0;
    }
}

pub(super) struct BadTestRef {
    site: HitSite,
    index: TestIndex,
}

impl BadTestRef {
    pub(super) fn into_error(self, path: &SourcePath, tests: usize) -> ModelError {
        return ModelError::TestIndexOutOfRange {
            path: path.clone(),
            site: self.site,
            index: self.index,
            tests,
        };
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<TestIndex>")]
pub struct HitSet(pub(super) Vec<TestIndex>);

impl HitSet {
    pub fn new(indexes: Vec<TestIndex>) -> Result<Self, ModelError> {
        ensure_ascending(
            &indexes,
            Collection::Hits,
            |previous, next| return previous < next,
            ToString::to_string,
        )?;
        return Ok(Self(indexes));
    }

    #[must_use]
    pub fn as_slice(&self) -> &[TestIndex] {
        return &self.0;
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        return self.0.is_empty();
    }

    fn resolve(
        mut self,
        refs: &impl TestRefs,
        site: impl FnOnce() -> HitSite,
    ) -> Result<Self, BadTestRef> {
        for slot in &mut self.0 {
            let index = *slot;
            let Some(target) = refs.resolve(index) else {
                return Err(BadTestRef {
                    site: site(),
                    index,
                });
            };
            *slot = target;
        }
        return Ok(self);
    }

    fn check(&self, tests: usize, site: impl FnOnce() -> HitSite) -> Result<(), BadTestRef> {
        let beyond = self
            .0
            .partition_point(|index| return index.position().is_some_and(|at| return at < tests));
        return self.0.get(beyond).map_or(Ok(()), |&index| {
            return Err(BadTestRef {
                site: site(),
                index,
            });
        });
    }
}

pub fn line_hits(
    executable: impl IntoIterator<Item = Line>,
    hits: impl IntoIterator<Item = (Line, TestIndex)>,
) -> Result<BTreeMap<Line, HitSet>, ModelError> {
    let mut lines: BTreeMap<Line, HitSet> = executable
        .into_iter()
        .map(|line| return (line, HitSet::default()))
        .collect();
    for (line, test) in hits {
        let Some(set) = lines.get_mut(&line) else {
            return Err(ModelError::HitOnNonExecutableLine { line, test });
        };
        set.0.push(test);
    }
    for set in lines.values_mut() {
        set.0.sort_unstable();
        set.0.dedup();
    }
    return Ok(lines);
}

impl TryFrom<Vec<TestIndex>> for HitSet {
    type Error = ModelError;

    fn try_from(indexes: Vec<TestIndex>) -> Result<Self, ModelError> {
        return Self::new(indexes);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SourcePath(String);

impl SourcePath {
    pub fn new(raw: String) -> Result<Self, ModelError> {
        let relative = raw.strip_prefix('/').unwrap_or(&raw);
        if !normalized(relative) {
            return Err(ModelError::InvalidPath { path: raw });
        }
        return Ok(Self(raw));
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        return &self.0;
    }

    #[must_use]
    pub fn is_absolute(&self) -> bool {
        return self.0.starts_with('/');
    }
}

impl fmt::Display for SourcePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(&self.0);
    }
}

fn normalized(relative: &str) -> bool {
    return !relative.is_empty()
        && relative
            .split('/')
            .all(|segment| return !matches!(segment, "" | "." | ".."));
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Root(String);

impl Root {
    pub fn new(raw: String) -> Result<Self, ModelError> {
        let valid = raw == "/" || raw.strip_prefix('/').is_some_and(normalized);
        if !valid {
            return Err(ModelError::InvalidRoot { root: raw });
        }
        return Ok(Self(raw));
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        return &self.0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathMode {
    Relative,
    Absolute,
}

impl fmt::Display for PathMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(match self {
            Self::Relative => "relative",
            Self::Absolute => "absolute",
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPaths", into = "RawPaths")]
pub enum PathBase {
    Relative(Root),
    Absolute,
}

impl PathBase {
    #[must_use]
    pub const fn mode(&self) -> PathMode {
        return match self {
            Self::Relative(_) => PathMode::Relative,
            Self::Absolute => PathMode::Absolute,
        };
    }

    pub fn locate(&self, compiled: &str) -> Result<SourcePath, ModelError> {
        let raw = match self {
            Self::Absolute => compiled,
            Self::Relative(root) => {
                let inside = if root.0 == "/" {
                    compiled.strip_prefix('/')
                } else {
                    compiled
                        .strip_prefix(root.0.as_str())
                        .and_then(|rest| return rest.strip_prefix('/'))
                };
                inside.ok_or_else(|| {
                    return ModelError::OutsideRoot {
                        path: compiled.to_owned(),
                        root: root.clone(),
                    };
                })?
            }
        };
        let path = SourcePath::new(raw.to_owned())?;
        self.admits(&path)?;
        return Ok(path);
    }

    pub(super) fn admits(&self, path: &SourcePath) -> Result<(), ModelError> {
        let mode = self.mode();
        if path.is_absolute() != (mode == PathMode::Absolute) {
            return Err(ModelError::PathModeMismatch {
                path: path.clone(),
                mode,
            });
        }
        return Ok(());
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPaths {
    mode: PathMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<String>,
}

impl TryFrom<RawPaths> for PathBase {
    type Error = ModelError;

    fn try_from(raw: RawPaths) -> Result<Self, ModelError> {
        return match (raw.mode, raw.root) {
            (PathMode::Relative, Some(root)) => Ok(Self::Relative(Root::new(root)?)),
            (PathMode::Relative, None) => Err(ModelError::MissingRoot),
            (PathMode::Absolute, Some(root)) => Err(ModelError::UnexpectedRoot { root }),
            (PathMode::Absolute, None) => Ok(Self::Absolute),
        };
    }
}

impl From<PathBase> for RawPaths {
    fn from(paths: PathBase) -> Self {
        let mode = paths.mode();
        let root = match paths {
            PathBase::Relative(root) => Some(root.0),
            PathBase::Absolute => None,
        };
        return Self { mode, root };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VmKind {
    Call,
    Switch,
    Goto,
    Hybrid,
    Tailcall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    pub opcov: String,
    pub php: String,
    pub zts: bool,
    pub vm: VmKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(deserialize_with = "ascending_set")]
    pub collect_paths: BTreeSet<String>,
    #[serde(deserialize_with = "ascending_set")]
    pub exclude_paths: BTreeSet<String>,
    #[serde(deserialize_with = "required")]
    pub exclude_pattern: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub generator: Generator,
    pub settings: Settings,
    pub paths: PathBase,
    pub tests: TestTable,
}

impl MapKey for FunctionKey {
    const COLLECTION: Collection = Collection::Functions;
    const ASCENDING: bool = false;

    fn parse_key(raw: String) -> Result<Self, ModelError> {
        return raw.parse().map_err(ModelError::FunctionKey);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id<const PREFIX: char>(u32);

pub type OutcomeId = Id<'o'>;
pub type LoopId = Id<'l'>;

impl<const PREFIX: char> Id<PREFIX> {
    #[must_use]
    pub const fn new(number: u32) -> Self {
        return Self(number);
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        return self.0;
    }

    pub fn parse(raw: &str) -> Result<Self, ModelError> {
        return raw
            .strip_prefix(PREFIX)
            .and_then(parse_canonical_u32)
            .map(Self)
            .ok_or_else(|| {
                return ModelError::InvalidId {
                    value: raw.to_owned(),
                    prefix: PREFIX,
                };
            });
    }
}

impl<const PREFIX: char> fmt::Display for Id<PREFIX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(formatter, "{PREFIX}{}", self.0);
    }
}

impl<const PREFIX: char> Serialize for Id<PREFIX> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.collect_str(self);
    }
}

impl<'de, const PREFIX: char> Deserialize<'de> for Id<PREFIX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        return Self::parse(&raw).map_err(de::Error::custom);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "EdgeKind")]
pub struct OutcomeKind(EdgeKind);

impl OutcomeKind {
    pub const fn new(kind: EdgeKind) -> Result<Self, ModelError> {
        if !kind.is_outcome() {
            return Err(ModelError::NotAnOutcome { kind });
        }
        return Ok(Self(kind));
    }

    #[must_use]
    pub const fn get(self) -> EdgeKind {
        return self.0;
    }
}

impl TryFrom<EdgeKind> for OutcomeKind {
    type Error = ModelError;

    fn try_from(kind: EdgeKind) -> Result<Self, ModelError> {
        return Self::new(kind);
    }
}

pub trait Identified {
    type Id: Ord + Copy + fmt::Display;
    const COLLECTION: Collection;
    fn id(&self) -> Self::Id;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "OutcomeRecord")]
pub struct Outcome {
    pub(super) id: OutcomeId,
    pub(super) site: OpIndex,
    pub(super) kind: OutcomeKind,
    pub(super) line: Line,
    pub(super) span: Option<Span>,
    pub(super) hits: HitSet,
}

impl Outcome {
    pub fn new(
        id: OutcomeId,
        site: OpIndex,
        kind: OutcomeKind,
        line: Line,
        span: Option<Span>,
        hits: HitSet,
    ) -> Result<Self, ModelError> {
        if let Some(span) = span
            && !span.contains_line(line)
        {
            return Err(ModelError::LineOutsideSpan { line, span });
        }
        return Ok(Self {
            id,
            site,
            kind,
            line,
            span,
            hits,
        });
    }

    #[must_use]
    pub const fn site(&self) -> OpIndex {
        return self.site;
    }

    #[must_use]
    pub const fn kind(&self) -> OutcomeKind {
        return self.kind;
    }

    #[must_use]
    pub const fn line(&self) -> Line {
        return self.line;
    }

    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        return self.span;
    }

    #[must_use]
    pub const fn hits(&self) -> &HitSet {
        return &self.hits;
    }
}

impl Identified for Outcome {
    type Id = OutcomeId;
    const COLLECTION: Collection = Collection::Outcomes;

    fn id(&self) -> OutcomeId {
        return self.id;
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutcomeRecord {
    id: OutcomeId,
    site: OpIndex,
    kind: OutcomeKind,
    line: Line,
    #[serde(deserialize_with = "required")]
    span: Option<Span>,
    hits: HitSet,
}

impl TryFrom<OutcomeRecord> for Outcome {
    type Error = ModelError;

    fn try_from(record: OutcomeRecord) -> Result<Self, ModelError> {
        return Self::new(
            record.id,
            record.site,
            record.kind,
            record.line,
            record.span,
            record.hits,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "LoopShape", into = "LoopShape")]
pub struct MeasuredShape(LoopShape);

impl MeasuredShape {
    pub const fn new(shape: LoopShape) -> Result<Self, ModelError> {
        if matches!(shape, LoopShape::Unstructured) {
            return Err(ModelError::UnstructuredLoop);
        }
        return Ok(Self(shape));
    }

    #[must_use]
    pub const fn get(self) -> LoopShape {
        return self.0;
    }
}

impl TryFrom<LoopShape> for MeasuredShape {
    type Error = ModelError;

    fn try_from(shape: LoopShape) -> Result<Self, ModelError> {
        return Self::new(shape);
    }
}

impl From<MeasuredShape> for LoopShape {
    fn from(kind: MeasuredShape) -> Self {
        return kind.0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub(super) id: LoopId,
    pub(super) site: OpIndex,
    pub(super) kind: MeasuredShape,
    pub(super) line: Line,
    #[serde(deserialize_with = "required")]
    pub(super) zero: Option<HitSet>,
    pub(super) second: HitSet,
}

impl Loop {
    #[must_use]
    pub const fn new(
        id: LoopId,
        site: OpIndex,
        kind: MeasuredShape,
        line: Line,
        zero: Option<HitSet>,
        second: HitSet,
    ) -> Self {
        return Self {
            id,
            site,
            kind,
            line,
            zero,
            second,
        };
    }

    #[must_use]
    pub const fn site(&self) -> OpIndex {
        return self.site;
    }

    #[must_use]
    pub const fn kind(&self) -> MeasuredShape {
        return self.kind;
    }

    #[must_use]
    pub const fn line(&self) -> Line {
        return self.line;
    }

    #[must_use]
    pub const fn zero(&self) -> Option<&HitSet> {
        return self.zero.as_ref();
    }

    #[must_use]
    pub const fn second(&self) -> &HitSet {
        return &self.second;
    }
}

impl Identified for Loop {
    type Id = LoopId;
    const COLLECTION: Collection = Collection::Loops;

    fn id(&self) -> LoopId {
        return self.id;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdSorted<T>(pub(super) Vec<T>);

impl<T: Identified> IdSorted<T> {
    pub fn new(items: Vec<T>) -> Result<Self, ModelError> {
        ensure_ascending(
            &items,
            T::COLLECTION,
            |previous, next| return previous.id() < next.id(),
            |item| return item.id().to_string(),
        )?;
        return Ok(Self(items));
    }
}

impl<T> IdSorted<T> {
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        return &self.0;
    }
}

impl<T> Default for IdSorted<T> {
    fn default() -> Self {
        return Self(Vec::new());
    }
}

impl<T: Serialize> Serialize for IdSorted<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return self.0.serialize(serializer);
    }
}

impl<'de, T: Deserialize<'de> + Identified> Deserialize<'de> for IdSorted<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let items = Vec::<T>::deserialize(deserializer)?;
        return Self::new(items).map_err(de::Error::custom);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Function {
    #[serde(deserialize_with = "required")]
    pub span: Option<Span>,
    pub outcomes: IdSorted<Outcome>,
    pub loops: IdSorted<Loop>,
}

impl Function {
    fn check_tests(&self, key: &FunctionKey, tests: usize) -> Result<(), BadTestRef> {
        for outcome in &self.outcomes.0 {
            outcome.hits.check(tests, || {
                return HitSite::Outcome {
                    function: key.clone(),
                    id: outcome.id,
                };
            })?;
        }
        for looped in &self.loops.0 {
            if let Some(zero) = &looped.zero {
                zero.check(tests, || {
                    return HitSite::LoopZero {
                        function: key.clone(),
                        id: looped.id,
                    };
                })?;
            }
            looped.second.check(tests, || {
                return HitSite::LoopSecond {
                    function: key.clone(),
                    id: looped.id,
                };
            })?;
        }
        return Ok(());
    }

    fn resolve_tests(self, key: &FunctionKey, refs: &impl TestRefs) -> Result<Self, BadTestRef> {
        let Self {
            span,
            outcomes,
            loops,
        } = self;
        let outcomes = outcomes
            .0
            .into_iter()
            .map(|outcome| {
                let id = outcome.id;
                let hits = outcome.hits.resolve(refs, || {
                    return HitSite::Outcome {
                        function: key.clone(),
                        id,
                    };
                })?;
                return Ok(Outcome { hits, ..outcome });
            })
            .collect::<Result<Vec<_>, BadTestRef>>()?;
        let loops = loops
            .0
            .into_iter()
            .map(|looped| {
                let id = looped.id;
                let zero = looped
                    .zero
                    .map(|zero| {
                        return zero.resolve(refs, || {
                            return HitSite::LoopZero {
                                function: key.clone(),
                                id,
                            };
                        });
                    })
                    .transpose()?;
                let second = looped.second.resolve(refs, || {
                    return HitSite::LoopSecond {
                        function: key.clone(),
                        id,
                    };
                })?;
                return Ok(Loop {
                    zero,
                    second,
                    ..looped
                });
            })
            .collect::<Result<Vec<_>, BadTestRef>>()?;
        return Ok(Self {
            span,
            outcomes: IdSorted(outcomes),
            loops: IdSorted(loops),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileCoverage {
    pub sha256: Sha256,
    #[serde(deserialize_with = "ascending_map")]
    pub lines: BTreeMap<Line, HitSet>,
    #[serde(deserialize_with = "ascending_map")]
    pub functions: BTreeMap<FunctionKey, Function>,
}

impl FileCoverage {
    fn check_tests(&self, tests: usize) -> Result<(), BadTestRef> {
        for (line, hits) in &self.lines {
            hits.check(tests, || return HitSite::Line(*line))?;
        }
        for (key, function) in &self.functions {
            function.check_tests(key, tests)?;
        }
        return Ok(());
    }

    pub(super) fn resolve_tests(self, refs: &impl TestRefs) -> Result<Self, BadTestRef> {
        let Self {
            sha256,
            lines,
            functions,
        } = self;
        let lines = lines
            .into_iter()
            .map(|(line, hits)| {
                let hits = hits.resolve(refs, || return HitSite::Line(line))?;
                return Ok((line, hits));
            })
            .collect::<Result<BTreeMap<_, _>, BadTestRef>>()?;
        let functions = functions
            .into_iter()
            .map(|(key, function)| {
                let function = function.resolve_tests(&key, refs)?;
                return Ok((key, function));
            })
            .collect::<Result<BTreeMap<_, _>, BadTestRef>>()?;
        return Ok(Self {
            sha256,
            lines,
            functions,
        });
    }
}

fn check_keys(paths: &PathBase, path: &SourcePath, file: &FileCoverage) -> Result<(), ModelError> {
    paths.admits(path)?;
    if let Some(key) = file.functions.keys().find(|key| {
        return key
            .file()
            .is_some_and(|named| return named.as_str() != path.as_str());
    }) {
        return Err(ModelError::KeyFileMismatch {
            path: path.clone(),
            key: Box::new(key.clone()),
        });
    }
    return Ok(());
}

pub(super) fn validate_entry(
    paths: &PathBase,
    refs: &impl TestRefs,
    path: &SourcePath,
    file: FileCoverage,
) -> Result<FileCoverage, ModelError> {
    check_keys(paths, path, &file)?;
    return file
        .resolve_tests(refs)
        .map_err(|bad| return bad.into_error(path, refs.count()));
}

pub(super) fn check_entry(
    paths: &PathBase,
    tests: usize,
    path: &SourcePath,
    file: &FileCoverage,
) -> Result<(), ModelError> {
    check_keys(paths, path, file)?;
    return file
        .check_tests(tests)
        .map_err(|bad| return bad.into_error(path, tests));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    header: Header,
    files: BTreeMap<SourcePath, FileCoverage>,
}

impl Report {
    pub fn new(
        header: Header,
        files: BTreeMap<SourcePath, FileCoverage>,
    ) -> Result<Self, ModelError> {
        for (path, file) in &files {
            check_entry(&header.paths, header.tests.len(), path, file)?;
        }
        return Ok(Self { header, files });
    }

    pub(super) const fn from_validated(
        header: Header,
        files: BTreeMap<SourcePath, FileCoverage>,
    ) -> Self {
        return Self { header, files };
    }

    #[must_use]
    pub const fn header(&self) -> &Header {
        return &self.header;
    }

    #[must_use]
    pub const fn files(&self) -> &BTreeMap<SourcePath, FileCoverage> {
        return &self.files;
    }
}
