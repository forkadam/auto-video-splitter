use std::io::{self, IsTerminal, Write};

use serde::Serialize;
use serde_json::{json, Map, Value};
use vsplit_core::actions::ActionError;
use vsplit_core::locks::LockError;

use crate::exit_codes;

#[derive(Debug, Clone)]
pub struct CliError {
    pub exit_code: i32,
    pub code: String,
    pub message: String,
    pub details: Value,
}

impl CliError {
    pub fn new(exit_code: i32, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            code: code.into(),
            message: message.into(),
            details: Value::Object(Map::new()),
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn usage(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(exit_codes::USAGE_ERROR, code, message)
    }

    pub fn runtime(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(exit_codes::RUNTIME_ERROR, code, message)
    }

    pub fn locked(slug: &str, lease: &vsplit_core::locks::LockLease) -> Self {
        Self::new(
            exit_codes::WORKSPACE_LOCKED,
            "WORKSPACE_LOCKED",
            format!(
                "workspace {slug:?} is locked by {} (pid {}, stage {})",
                lease.actor.as_str(),
                lease.pid,
                lease.stage
            ),
        )
        .with_details(json!({
            "slug": slug,
            "locked_by": lease,
        }))
    }

    pub fn from_action(err: ActionError) -> Self {
        match err {
            ActionError::Lock(LockError::Held { slug, lease }) => Self::locked(&slug, &lease),
            ActionError::HashMismatch { expected, actual } => Self::usage(
                "HASH_MISMATCH",
                format!("cuts.json was modified externally (expected {expected}, actual {actual})"),
            )
            .with_details(json!({ "expected_hash": expected, "actual_hash": actual })),
            ActionError::Path(e) => Self::usage("INVALID_PATH", e.to_string()),
            ActionError::Schema(e) => Self::usage("SCHEMA_VALIDATION_ERROR", e.to_string()),
            ActionError::UnknownStage(s) => {
                Self::usage("UNKNOWN_STAGE", format!("unknown stage: {s:?}"))
            }
            ActionError::Validation(msg) => Self::usage("VALIDATION_ERROR", msg),
            ActionError::CutsNotFound(s) => {
                Self::runtime("NOT_FOUND", format!("cuts.json not found for {s:?}"))
            }
            ActionError::LogNotFound { slug, stage } => {
                Self::runtime("NOT_FOUND", format!("log not found for {slug}/{stage}"))
            }
            other => Self::runtime("ACTION_FAILED", other.to_string()),
        }
    }
}

/// Emit a structured `{ "ok": true, ... }` JSON object to stdout in `--json` mode,
/// or invoke `human` for human-readable output.
pub fn emit_ok<T: Serialize, F: FnOnce()>(json_mode: bool, payload: &T, human: F) {
    if json_mode {
        let mut obj = match serde_json::to_value(payload).unwrap_or(Value::Null) {
            Value::Object(m) => m,
            other => {
                let mut m = Map::new();
                m.insert("data".to_string(), other);
                m
            }
        };
        obj.insert("ok".to_string(), Value::Bool(true));
        println!(
            "{}",
            serde_json::to_string_pretty(&Value::Object(obj))
                .unwrap_or_else(|_| "{\"ok\":true}".into())
        );
    } else {
        human();
    }
}

/// Emit `{ "ok": false, "code": ..., "message": ..., "details": ... }` to stdout under `--json`,
/// or a human-readable diagnostic to stderr otherwise.
pub fn emit_err(json_mode: bool, err: &CliError) {
    if json_mode {
        let payload = json!({
            "ok": false,
            "code": err.code,
            "message": err.message,
            "details": err.details,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{\"ok\":false}".into())
        );
    } else {
        eprintln!("error [{}]: {}", err.code, err.message);
        if let Some(hint) = err.details.get("hint").and_then(Value::as_str) {
            eprintln!("hint: {hint}");
        }
    }
}

/// Interactive confirmation guard: never blocks when stdin is not a TTY or `--non-interactive` is set.
pub fn confirm_or_fail_fast(
    prompt: &str,
    non_interactive: bool,
    hint: &str,
) -> Result<bool, CliError> {
    if non_interactive || !io::stdin().is_terminal() {
        return Err(CliError::usage(
            "NON_INTERACTIVE_CONFIRM_REQUIRED",
            format!("confirmation required ({prompt}), but running non-interactively"),
        )
        .with_details(json!({ "hint": hint })));
    }
    eprint!("{prompt} [y/N]: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| CliError::runtime("STDIN_ERROR", e.to_string()))?;
    let ans = line.trim().to_ascii_lowercase();
    Ok(ans == "y" || ans == "yes")
}
