//! The v1 tool set. Every tool is a thin adapter: validate args, call
//! the host bridge, shape the reply for the model. No filesystem access
//! here — the host (the editor) does the I/O.

pub mod edit;
pub mod find;
pub mod grep;
pub mod read;
pub mod write;

use std::path::PathBuf;

use serde_json::Value;

/// Extract a required string argument.
pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument '{key}'"))
}

/// Extract an optional usize argument.
pub fn arg_usize(args: &Value, key: &str) -> Result<Option<usize>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(|n| Some(n as usize))
            .ok_or_else(|| format!("argument '{key}' must be a positive integer")),
    }
}

/// Extract a required path argument.
pub fn arg_path(args: &Value, key: &str) -> Result<PathBuf, String> {
    arg_str(args, key).map(PathBuf::from)
}
