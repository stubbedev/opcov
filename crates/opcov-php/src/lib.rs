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
mod canonical;
mod consumers;
mod declaration;
mod facts;
mod graph;
mod hierarchy;
mod index;
mod inferred;
mod laravel;
mod live;
mod matching;
mod members;
mod project;
mod references;
mod routes;
mod sites;
mod syntax;
mod target;
mod trait_scope;
mod tree;
mod types;
mod unaffected;

pub use cache::{CacheError, ScanCache};
pub use consumers::{Search, Untyped, Use};
pub use graph::{CoverageTarget, Graph, GraphError, Links, NodeCoverage, Ratio, Revision};
pub use index::{Build, Index, IndexError, IndexStats};
pub use live::Refresh;
pub use members::Member;
pub use project::{Overlaid, Project, ProjectError};
pub use routes::{Route, Routes, RoutesError};
pub use target::{Kind, Request, Resolution, Target, TargetError};
pub use tree::{Answer, Changed, Consumer, FileChange, Query, Tree};
