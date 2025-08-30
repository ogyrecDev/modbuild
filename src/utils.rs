// modbuild/src/utils.rs

use std::env;
use std::process::Command;

pub fn has_cmd(cmd: &str, arg: &str) -> bool {
  Command::new(cmd)
    .arg(arg)
    .output()
    .map(|o| o.status.success())
    .unwrap_or(false)
}

/// Is `cargo zigbuild` available on this machine?
pub fn has_zigbuild() -> bool {
  has_cmd("cargo", "zigbuild")
}

/// Is the requested target equal to the host toolchain triple?
pub fn is_host_triple(triple: &str) -> bool {
  let host = env::var("HOST").unwrap_or_default();
  host == triple
}

/// Checks if a macOS compiler is available
pub fn has_mac_compiler() -> bool {
  if let Ok(path) = env::var("CC") {
    let p = path.to_lowercase();
    return p.contains("o64-clang") || p.contains("oa64-clang") || p.contains("zig");
  }
  has_cmd("which", "zig") || has_cmd("which", "o64-clang") || has_cmd("which", "oa64-clang")
}
