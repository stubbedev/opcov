use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

use super::PathSetting;

#[derive(Debug)]
pub enum FilterError {
    MissingListfileName {
        setting: PathSetting,
    },
    ListfileUnreadable {
        setting: PathSetting,
        listfile: PathBuf,
        source: io::Error,
    },
    EmptyListfile {
        setting: PathSetting,
        listfile: PathBuf,
    },
    EntryUnresolvable {
        setting: PathSetting,
        entry: PathBuf,
        source: io::Error,
    },
    NotCanonical {
        setting: PathSetting,
        entry: PathBuf,
        resolved: PathBuf,
    },
}

impl FilterError {
    #[must_use]
    pub const fn setting(&self) -> PathSetting {
        return match self {
            Self::MissingListfileName { setting }
            | Self::ListfileUnreadable { setting, .. }
            | Self::EmptyListfile { setting, .. }
            | Self::EntryUnresolvable { setting, .. }
            | Self::NotCanonical { setting, .. } => *setting,
        };
    }
}

impl fmt::Display for FilterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let setting = self.setting().ini_name();
        return match self {
            Self::MissingListfileName { .. } => {
                write!(formatter, "{setting}: `@` must be followed by the path of a listfile")
            }
            Self::ListfileUnreadable {
                listfile, source, ..
            } => write!(
                formatter,
                "{setting}: cannot read listfile {}: {source}",
                listfile.display()
            ),
            Self::EmptyListfile { listfile, .. } => write!(
                formatter,
                "{setting}: listfile {} contains no paths",
                listfile.display()
            ),
            Self::EntryUnresolvable { entry, source, .. } => write!(
                formatter,
                "{setting}: cannot resolve {}: {source}",
                entry.display()
            ),
            Self::NotCanonical {
                entry, resolved, ..
            } => write!(
                formatter,
                "{setting}: {} resolved to {}, which is not an absolute path without `.` or `..` segments",
                entry.display(),
                resolved.display()
            ),
        };
    }
}

impl Error for FilterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        return match self {
            Self::ListfileUnreadable { source, .. } | Self::EntryUnresolvable { source, .. } => {
                Some(source)
            }
            Self::MissingListfileName { .. }
            | Self::EmptyListfile { .. }
            | Self::NotCanonical { .. } => None,
        };
    }
}
