/// Deterministic CLI exit codes per design doc §5.
pub const OK: i32 = 0;
pub const RUNTIME_ERROR: i32 = 1;
pub const USAGE_ERROR: i32 = 2;
pub const WORKSPACE_LOCKED: i32 = 3;
pub const MISSING_DEPENDENCY: i32 = 4;
pub const AWAITING_USER: i32 = 10;
pub const INTERRUPTED: i32 = 130;
