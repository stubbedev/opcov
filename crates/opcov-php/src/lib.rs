#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "rustc's unreachable_pub, which the workspace denies, requires pub(crate) in private modules"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "error conditions are carried by the precise error types, and the source has no comments"
)]

mod analysis;
mod cache;
mod consumers;
mod facts;
mod hierarchy;
mod index;
mod laravel;
mod live;
mod matching;
mod members;
mod project;
mod references;
mod sites;
mod syntax;
mod target;
mod tree;
mod types;

pub use cache::{CacheError, ScanCache};
pub use consumers::{Search, Untyped, Use};
pub use index::{Build, Index, IndexError, IndexStats};
pub use live::Refresh;
pub use members::Member;
pub use project::{Project, ProjectError};
pub use target::{Kind, Request, Target, TargetError};
pub use tree::{Answer, Changed, Consumer, FileChange, Query, Tree};
