use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::model::Line;

const MAIN_PREFIX: &str = "{main}@";
const CLOSURE_PREFIX: &str = "{closure}@";
const ARROW_FUNCTION_PREFIX: &str = "{fn}@";
const ANONYMOUS_CLASS_PREFIX: &str = "class@anonymous@";
const MEMBER_SEPARATOR: &str = "::";
const NAMESPACE_SEPARATOR: char = '\\';
const PROPERTY_SIGIL: char = '$';

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyErrorReason {
    EmptyFilePath,
    NulInFilePath,
    InvalidIdentifier,
    InvalidQualifiedName,
    MissingLineSpan,
    InvalidLineNumber,
    InvertedLineSpan,
}

impl fmt::Display for KeyErrorReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::EmptyFilePath => "the file path is empty",
            Self::NulInFilePath => "the file path contains a NUL byte",
            Self::InvalidIdentifier => "a name is not a valid PHP identifier",
            Self::InvalidQualifiedName => "a name is not a valid namespaced PHP name",
            Self::MissingLineSpan => "the `:<start>-<end>` line span is missing",
            Self::InvalidLineNumber => "a line number is not a canonical positive integer",
            Self::InvertedLineSpan => "the line span ends before it starts",
        };
        return f.write_str(text);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyError {
    input: String,
    reason: KeyErrorReason,
}

impl KeyError {
    fn new(input: &str, reason: KeyErrorReason) -> Self {
        return Self {
            input: input.to_owned(),
            reason,
        };
    }

    fn within(self, input: &str) -> Self {
        return Self::new(input, self.reason);
    }

    #[must_use]
    pub fn input(&self) -> &str {
        return &self.input;
    }

    #[must_use]
    pub const fn reason(&self) -> KeyErrorReason {
        return self.reason;
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "cannot parse `{}`: {}", self.input, self.reason);
    }
}

impl std::error::Error for KeyError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct FilePath(String);

impl FilePath {
    #[must_use]
    pub fn as_str(&self) -> &str {
        return &self.0;
    }
}

impl TryFrom<String> for FilePath {
    type Error = KeyError;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        if path.is_empty() {
            return Err(KeyError::new(&path, KeyErrorReason::EmptyFilePath));
        }
        if path.contains('\0') {
            return Err(KeyError::new(&path, KeyErrorReason::NulInFilePath));
        }
        return Ok(Self(path));
    }
}

impl FromStr for FilePath {
    type Err = KeyError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        return Self::try_from(path.to_owned());
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.write_str(&self.0);
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let starts_well = first == '_' || first.is_ascii_alphabetic() || !first.is_ascii();
    return starts_well
        && chars.all(|c| return c == '_' || c.is_ascii_alphanumeric() || !c.is_ascii());
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Identifier(String);

impl Identifier {
    #[must_use]
    pub fn as_str(&self) -> &str {
        return &self.0;
    }
}

impl FromStr for Identifier {
    type Err = KeyError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if !is_identifier(name) {
            return Err(KeyError::new(name, KeyErrorReason::InvalidIdentifier));
        }
        return Ok(Self(name.to_owned()));
    }
}

impl TryFrom<String> for Identifier {
    type Error = KeyError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        return name.parse();
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.write_str(&self.0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct QualifiedName(String);

impl QualifiedName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        return &self.0;
    }
}

impl FromStr for QualifiedName {
    type Err = KeyError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if !name.split(NAMESPACE_SEPARATOR).all(is_identifier) {
            return Err(KeyError::new(name, KeyErrorReason::InvalidQualifiedName));
        }
        return Ok(Self(name.to_owned()));
    }
}

impl TryFrom<String> for QualifiedName {
    type Error = KeyError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        return name.parse();
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.write_str(&self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineSpan {
    start: Line,
    end: Line,
}

impl LineSpan {
    #[must_use]
    pub fn new(start: Line, end: Line) -> Option<Self> {
        return (start <= end).then_some(Self { start, end });
    }

    #[must_use]
    pub const fn start(self) -> Line {
        return self.start;
    }

    #[must_use]
    pub const fn end(self) -> Line {
        return self.end;
    }
}

impl fmt::Display for LineSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{}-{}", self.start.get(), self.end.get());
    }
}

fn parse_line(text: &str) -> Result<Line, KeyError> {
    let canonical = !text.starts_with('0')
        && !text.is_empty()
        && text.bytes().all(|b| return b.is_ascii_digit());
    return text
        .parse::<u32>()
        .ok()
        .filter(|_| return canonical)
        .and_then(Line::new)
        .ok_or_else(|| return KeyError::new(text, KeyErrorReason::InvalidLineNumber));
}

impl FromStr for LineSpan {
    type Err = KeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let Some((start, end)) = text.split_once('-') else {
            return Err(KeyError::new(text, KeyErrorReason::MissingLineSpan));
        };
        let start = parse_line(start)?;
        let end = parse_line(end)?;
        return Self::new(start, end)
            .ok_or_else(|| return KeyError::new(text, KeyErrorReason::InvertedLineSpan));
    }
}

fn parse_located(text: &str) -> Result<(FilePath, LineSpan), KeyError> {
    let Some((file, lines)) = text.rsplit_once(':') else {
        return Err(KeyError::new(text, KeyErrorReason::MissingLineSpan));
    };
    return Ok((file.parse()?, lines.parse()?));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HookKind {
    Get,
    Set,
}

impl HookKind {
    const fn as_str(self) -> &'static str {
        return match self {
            Self::Get => "get",
            Self::Set => "set",
        };
    }

    fn parse(text: &str) -> Option<Self> {
        return [Self::Get, Self::Set]
            .into_iter()
            .find(|hook| return hook.as_str() == text);
    }
}

impl fmt::Display for HookKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.write_str(self.as_str());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClassRef {
    Named(QualifiedName),
    Anonymous { file: FilePath, lines: LineSpan },
}

impl FromStr for ClassRef {
    type Err = KeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(located) = text.strip_prefix(ANONYMOUS_CLASS_PREFIX) {
            let (file, lines) = parse_located(located)?;
            return Ok(Self::Anonymous { file, lines });
        }
        return Ok(Self::Named(text.parse()?));
    }
}

impl fmt::Display for ClassRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::Anonymous { file, lines } => write!(f, "{ANONYMOUS_CLASS_PREFIX}{file}:{lines}"),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FunctionKey {
    Main {
        file: FilePath,
    },
    Function {
        name: QualifiedName,
    },
    Method {
        class: ClassRef,
        method: Identifier,
    },
    PropertyHook {
        class: ClassRef,
        property: Identifier,
        hook: HookKind,
    },
    Closure {
        file: FilePath,
        lines: LineSpan,
    },
    ArrowFunction {
        file: FilePath,
        lines: LineSpan,
    },
}

fn parse_hook(owner: &str, member: &str) -> Option<Result<FunctionKey, KeyError>> {
    let hook = HookKind::parse(member)?;
    let (class, property) = owner.rsplit_once(MEMBER_SEPARATOR)?;
    let property = property
        .strip_prefix(PROPERTY_SIGIL)?
        .parse::<Identifier>()
        .ok()?;
    return Some(class.parse().map(|class| {
        return FunctionKey::PropertyHook {
            class,
            property,
            hook,
        };
    }));
}

fn parse_key(text: &str) -> Result<FunctionKey, KeyError> {
    if let Some(file) = text.strip_prefix(MAIN_PREFIX) {
        return Ok(FunctionKey::Main {
            file: file.parse()?,
        });
    }
    if let Some(located) = text.strip_prefix(CLOSURE_PREFIX) {
        let (file, lines) = parse_located(located)?;
        return Ok(FunctionKey::Closure { file, lines });
    }
    if let Some(located) = text.strip_prefix(ARROW_FUNCTION_PREFIX) {
        let (file, lines) = parse_located(located)?;
        return Ok(FunctionKey::ArrowFunction { file, lines });
    }
    let Some((owner, member)) = text.rsplit_once(MEMBER_SEPARATOR) else {
        return Ok(FunctionKey::Function {
            name: text.parse()?,
        });
    };
    if let Some(hook) = parse_hook(owner, member) {
        return hook;
    }
    return Ok(FunctionKey::Method {
        class: owner.parse()?,
        method: member.parse()?,
    });
}

impl FromStr for FunctionKey {
    type Err = KeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        return parse_key(text).map_err(|error| return error.within(text));
    }
}

impl fmt::Display for FunctionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Main { file } => write!(f, "{MAIN_PREFIX}{file}"),
            Self::Function { name } => write!(f, "{name}"),
            Self::Method { class, method } => write!(f, "{class}{MEMBER_SEPARATOR}{method}"),
            Self::PropertyHook {
                class,
                property,
                hook,
            } => {
                write!(
                    f,
                    "{class}{MEMBER_SEPARATOR}{PROPERTY_SIGIL}{property}{MEMBER_SEPARATOR}{hook}"
                )
            }
            Self::Closure { file, lines } => write!(f, "{CLOSURE_PREFIX}{file}:{lines}"),
            Self::ArrowFunction { file, lines } => {
                write!(f, "{ARROW_FUNCTION_PREFIX}{file}:{lines}")
            }
        };
    }
}

impl Serialize for FunctionKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.collect_str(self);
    }
}

impl<'de> Deserialize<'de> for FunctionKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        return text.parse().map_err(de::Error::custom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(number: u32) -> Line {
        return Line::new(number).unwrap();
    }

    fn span(start: u32, end: u32) -> LineSpan {
        return LineSpan::new(line(start), line(end)).unwrap();
    }

    fn file(path: &str) -> FilePath {
        return path.parse().unwrap();
    }

    fn named(name: &str) -> ClassRef {
        return ClassRef::Named(name.parse().unwrap());
    }

    #[expect(clippy::too_many_lines, reason = "one table row per key shape")]
    fn canonical_keys() -> Vec<(FunctionKey, &'static str)> {
        return vec![
            (
                FunctionKey::Main {
                    file: file("/app/index.php"),
                },
                "{main}@/app/index.php",
            ),
            (
                FunctionKey::Function {
                    name: "strlen_safe".parse().unwrap(),
                },
                "strlen_safe",
            ),
            (
                FunctionKey::Function {
                    name: "App\\Support\\helper".parse().unwrap(),
                },
                "App\\Support\\helper",
            ),
            (
                FunctionKey::Method {
                    class: named("App\\Foo"),
                    method: "bar".parse().unwrap(),
                },
                "App\\Foo::bar",
            ),
            (
                FunctionKey::Method {
                    class: named("Foo"),
                    method: "get".parse().unwrap(),
                },
                "Foo::get",
            ),
            (
                FunctionKey::Method {
                    class: named("App\\Concerns\\HasName"),
                    method: "name".parse().unwrap(),
                },
                "App\\Concerns\\HasName::name",
            ),
            (
                FunctionKey::PropertyHook {
                    class: named("App\\User"),
                    property: "email".parse().unwrap(),
                    hook: HookKind::Get,
                },
                "App\\User::$email::get",
            ),
            (
                FunctionKey::PropertyHook {
                    class: named("User"),
                    property: "get".parse().unwrap(),
                    hook: HookKind::Set,
                },
                "User::$get::set",
            ),
            (
                FunctionKey::Closure {
                    file: file("/app/src/Foo.php"),
                    lines: span(12, 15),
                },
                "{closure}@/app/src/Foo.php:12-15",
            ),
            (
                FunctionKey::ArrowFunction {
                    file: file("/app/src/Foo.php"),
                    lines: span(7, 7),
                },
                "{fn}@/app/src/Foo.php:7-7",
            ),
            (
                FunctionKey::Closure {
                    file: file("/odd:dir/a-b.php"),
                    lines: span(1, 2),
                },
                "{closure}@/odd:dir/a-b.php:1-2",
            ),
            (
                FunctionKey::Method {
                    class: ClassRef::Anonymous {
                        file: file("/app/t.php"),
                        lines: span(3, 9),
                    },
                    method: "handle".parse().unwrap(),
                },
                "class@anonymous@/app/t.php:3-9::handle",
            ),
            (
                FunctionKey::PropertyHook {
                    class: ClassRef::Anonymous {
                        file: file("/x::$y.php"),
                        lines: span(3, 9),
                    },
                    property: "v".parse().unwrap(),
                    hook: HookKind::Get,
                },
                "class@anonymous@/x::$y.php:3-9::$v::get",
            ),
            (
                FunctionKey::Method {
                    class: ClassRef::Anonymous {
                        file: file("/a::$b"),
                        lines: span(1, 2),
                    },
                    method: "get".parse().unwrap(),
                },
                "class@anonymous@/a::$b:1-2::get",
            ),
            (
                FunctionKey::Function {
                    name: "ünïcode_fn".parse().unwrap(),
                },
                "ünïcode_fn",
            ),
        ];
    }

    #[test]
    fn canonical_strings_round_trip() {
        for (key, text) in canonical_keys() {
            assert_eq!(key.to_string(), text, "display of {key:?}");
            assert_eq!(text.parse::<FunctionKey>().unwrap(), key, "parse of {text}");
        }
    }

    #[test]
    fn keys_serialize_as_their_canonical_string() {
        for (key, text) in canonical_keys() {
            let json = serde_json::to_value(&key).unwrap();
            assert_eq!(
                json,
                serde_json::Value::String(text.to_owned()),
                "serialized {key:?}"
            );
            assert_eq!(
                serde_json::from_value::<FunctionKey>(json).unwrap(),
                key,
                "deserialized {text}"
            );
        }
    }

    #[test]
    fn malformed_keys_are_rejected_with_the_reason() {
        let cases = [
            ("", KeyErrorReason::InvalidQualifiedName),
            ("1abc", KeyErrorReason::InvalidQualifiedName),
            ("\\Foo\\bar", KeyErrorReason::InvalidQualifiedName),
            ("Foo\\", KeyErrorReason::InvalidQualifiedName),
            ("Foo::", KeyErrorReason::InvalidIdentifier),
            ("Foo::ba-r", KeyErrorReason::InvalidIdentifier),
            ("::bar", KeyErrorReason::InvalidQualifiedName),
            ("{main}@", KeyErrorReason::EmptyFilePath),
            ("{main}@/a\0b", KeyErrorReason::NulInFilePath),
            ("{closure}@/a.php", KeyErrorReason::MissingLineSpan),
            ("{closure}@/a.php:12", KeyErrorReason::MissingLineSpan),
            ("{closure}@:1-2", KeyErrorReason::EmptyFilePath),
            ("{closure}@/a.php:0-2", KeyErrorReason::InvalidLineNumber),
            ("{closure}@/a.php:01-2", KeyErrorReason::InvalidLineNumber),
            ("{closure}@/a.php:+1-2", KeyErrorReason::InvalidLineNumber),
            ("{closure}@/a.php:1-x", KeyErrorReason::InvalidLineNumber),
            ("{fn}@/a.php:5-4", KeyErrorReason::InvertedLineSpan),
            ("class@anonymous@/a.php::m", KeyErrorReason::MissingLineSpan),
            ("Foo::$bar::get::x", KeyErrorReason::InvalidQualifiedName),
        ];
        for (text, reason) in cases {
            let error = text.parse::<FunctionKey>().unwrap_err();
            assert_eq!(error.reason(), reason, "reason for {text:?}");
            assert_eq!(error.input(), text, "error input for {text:?}");
        }
    }

    #[test]
    fn hook_suffix_without_property_sigil_is_a_method() {
        let key = "Foo::bar::get".parse::<FunctionKey>().unwrap_err();
        assert_eq!(
            key.reason(),
            KeyErrorReason::InvalidQualifiedName,
            "Foo::bar is not a class name"
        );
    }

    #[test]
    fn line_span_requires_ordered_lines() {
        assert!(
            LineSpan::new(line(3), line(2)).is_none(),
            "inverted span must be rejected"
        );
        assert_eq!(span(2, 3).to_string(), "2-3", "span display");
    }
}
