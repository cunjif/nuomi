//! Out-of-kernel domain services for the UI M1 surfaces: sandboxed
//! workspace file access, whitelisted git subprocesses, scheduler.
//! Each service owns its own `thiserror` error type.

pub mod git_service;
pub mod scheduler_service;
pub mod workspace;

pub use git_service::{CommitInfo, GitError, GitService, StatusEntry, WorktreeInfo};
pub use scheduler_service::{
    next_after, parse_schedule, CronExpr, FieldSet, ScheduleSpec, SchedulerError, SchedulerHandle,
    SchedulerRunner,
};
pub use workspace::{FileEntry, WorkspaceError, WorkspaceService};
