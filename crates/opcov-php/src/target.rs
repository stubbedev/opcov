use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Method,
    Constant,
    Property,
    Function,
    GlobalConstant,
}

impl Kind {
    const fn name(self) -> &'static str {
        return match self {
            Self::Method => "method",
            Self::Constant => "constant",
            Self::Property => "property",
            Self::Function => "function",
            Self::GlobalConstant => "global-constant",
        };
    }

    fn parse(name: &str) -> Option<Self> {
        return [
            Self::Method,
            Self::Constant,
            Self::Property,
            Self::Function,
            Self::GlobalConstant,
        ]
        .into_iter()
        .find(|kind| return kind.name() == name);
    }

    pub(crate) const fn is_global(self) -> bool {
        return matches!(self, Self::Function | Self::GlobalConstant);
    }

    pub(crate) fn member_key(self, member: &str) -> String {
        return match self {
            Self::Method | Self::Function | Self::GlobalConstant => member.to_ascii_lowercase(),
            Self::Constant | Self::Property => member.to_owned(),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Target {
    pub kind: Kind,
    pub class: Option<String>,
    pub member: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetError {
    pub text: String,
}

impl fmt::Display for TargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "invalid target {:?}: expected `<kind> <Class>::<member>` for method, constant and property, or `<kind> <name>` for function and global-constant",
            self.text
        );
    }
}

impl std::error::Error for TargetError {}

impl FromStr for Target {
    type Err = TargetError;

    fn from_str(text: &str) -> Result<Self, TargetError> {
        let invalid = || {
            return TargetError {
                text: text.to_owned(),
            };
        };
        let (kind, name) = text.split_once(' ').ok_or_else(invalid)?;
        let kind = Kind::parse(kind).ok_or_else(invalid)?;
        let name = name.trim_start_matches('\\');
        if kind.is_global() {
            return if name.is_empty() || name.contains("::") {
                Err(invalid())
            } else {
                Ok(Self {
                    kind,
                    class: None,
                    member: name.to_owned(),
                })
            };
        }
        let (class, member) = name
            .split_once("::")
            .filter(|(class, member)| return !class.is_empty() && !member.is_empty())
            .ok_or_else(invalid)?;
        return Ok(Self {
            kind,
            class: Some(class.to_owned()),
            member: member.to_owned(),
        });
    }
}

impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match &self.class {
            Some(class) => write!(formatter, "{} {class}::{}", self.kind.name(), self.member),
            None => write!(formatter, "{} {}", self.kind.name(), self.member),
        };
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        return text.parse().map_err(serde::de::Error::custom);
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEvent {
    pub target: Target,
    pub model: String,
    pub instance: Vec<String>,
    pub query: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Command {
    pub target: Target,
    pub class: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub targets: Vec<Target>,
    #[serde(default)]
    pub queue_entry_methods: Vec<String>,
    #[serde(default)]
    pub model_event_targets: Vec<ModelEvent>,
    #[serde(default)]
    pub command_targets: Vec<Command>,
    #[serde(default)]
    pub within: Vec<String>,
}

impl Request {
    pub(crate) fn searches(&self, path: &str) -> bool {
        return self.within.is_empty()
            || self.within.iter().any(|root| {
                let root = root.trim_end_matches('/');
                return path == root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|rest| return rest.starts_with('/'));
            });
    }
}
