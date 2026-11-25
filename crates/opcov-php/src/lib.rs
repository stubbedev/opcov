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
mod consumers;
mod index;
mod laravel;
mod matching;
mod project;
mod references;
mod sites;
mod syntax;
mod target;
mod types;

pub use consumers::{Search, Use};
pub use index::{Index, IndexError, IndexStats};
pub use project::{Project, ProjectError};
pub use target::{Kind, Request, Target, TargetError};
