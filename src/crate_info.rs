// modbuild/src/crate_info.rs

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

/// Get crate name from Cargo metadata (workspace-safe)
pub fn get_crate_name(mod_path: &PathBuf) -> Result<String, String> {
  let output = Command::new("cargo")
    .args(["metadata", "--format-version", "1", "--no-deps"])
    .current_dir(mod_path)
    .output()
    .map_err(|e| format!("Failed to run cargo metadata: {e}"))?;

  if !output.status.success() {
    let err = String::from_utf8_lossy(&output.stderr);
    return Err(format!("cargo metadata failed:\n{}", err));
  }

  let stdout = String::from_utf8(output.stdout)
    .map_err(|e| format!("Invalid UTF-8 from cargo metadata: {e}"))?;

  let metadata: Value = serde_json::from_str(&stdout)
    .map_err(|e| format!("Failed to parse cargo metadata JSON: {e}"))?;

  let manifest_path = mod_path
    .join("Cargo.toml")
    .canonicalize()
    .unwrap_or(mod_path.join("Cargo.toml"));
  let manifest_str = manifest_path.to_string_lossy();

  let packages = metadata["packages"].as_array().ok_or("No packages")?;
  let pkg = packages
    .iter()
    .find(|p| {
      p["manifest_path"]
        .as_str()
        .map(|s| s == manifest_str)
        .unwrap_or(false)
    })
    .or_else(|| packages.first())
    .ok_or("No package found")?;

  let name = pkg["name"].as_str().ok_or("Failed to get package name")?;
  Ok(name.to_string())
}

/// Ensure the crate is configured to build a dynamic library
pub fn ensure_cdylib(mod_path: &PathBuf) -> Result<(), String> {
  let out = Command::new("cargo")
    .args(["read-manifest"])
    .current_dir(mod_path)
    .output()
    .map_err(|e| format!("Failed to run cargo read-manifest: {e}"))?;
  if !out.status.success() {
    return Err("cargo read-manifest failed".into());
  }
  let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
  let targets = v["targets"].as_array().ok_or("No targets in manifest")?;
  let has_cdylib = targets.iter().any(|t| {
    t["kind"]
      .as_array()
      .map(|ks| ks.iter().any(|k| k == "cdylib"))
      .unwrap_or(false)
  });
  if !has_cdylib {
    return Err("Your Cargo.toml must include:\n\n[lib]\ncrate-type = [\"cdylib\"]\n".into());
  }
  Ok(())
}
