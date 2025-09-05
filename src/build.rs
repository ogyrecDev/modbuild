// modbuild/src/build.rs

use std::fs;
use std::io::BufRead;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::utils::{has_mac_compiler, has_zigbuild, is_host_triple};

/// Represents a build target (OS + architecture)
#[derive(Clone)]
pub struct BuildTarget {
  pub name: &'static str,
  pub triple: &'static str,
  pub ext: &'static str,
  pub needs_mac: bool,
}

/// All supported targets (добавлен android-arm64)
pub fn all_targets() -> Vec<BuildTarget> {
  vec![
    BuildTarget {
      name: "linux",
      triple: "x86_64-unknown-linux-gnu",
      ext: "so",
      needs_mac: false,
    },
    BuildTarget {
      name: "windows-gnu",
      triple: "x86_64-pc-windows-gnu",
      ext: "dll",
      needs_mac: false,
    },
    BuildTarget {
      name: "windows-msvc",
      triple: "x86_64-pc-windows-msvc",
      ext: "dll",
      needs_mac: false,
    },
    BuildTarget {
      name: "mac-intel",
      triple: "x86_64-apple-darwin",
      ext: "dylib",
      needs_mac: true,
    },
    BuildTarget {
      name: "mac-arm64",
      triple: "aarch64-apple-darwin",
      ext: "dylib",
      needs_mac: true,
    },
    BuildTarget {
      name: "android-arm64",
      triple: "aarch64-linux-android",
      ext: "so",
      needs_mac: false,
    },
  ]
}

/// Select targets based on optional filter string
pub fn select_targets(filter: Option<String>) -> Vec<BuildTarget> {
  let all = all_targets();
  if let Some(f) = filter {
    f.split(',')
      .filter_map(|name| all.iter().find(|t| t.name == name.trim()))
      .cloned()
      .collect()
  } else {
    all
  }
}

/// Build a crate for a specific target
pub fn build_for_target(
  out: &PathBuf,
  target: &BuildTarget,
  mod_path: &PathBuf,
) -> Result<(), String> {
  // --- macOS ограничения для кросс-компиляции ---
  if target.needs_mac && !has_mac_compiler() {
    return Err(format!("Skipping {}: mac compiler not found", target.name));
  }

  #[cfg(target_os = "macos")]
  if target.triple == "x86_64-pc-windows-msvc" {
    return Err("Skipping windows-msvc: cannot cross-compile MSVC from macOS".into());
  }

  // --- Android: используем cargo-ndk с абсолютным -o и перекладываем .so в native/android-arm64 ---
  if target.name == "android-arm64" {
    if !has_cargo_ndk() {
      return Err("cargo-ndk not found. Install: `cargo install cargo-ndk`".into());
    }

    println!("Building for {} (via cargo-ndk)...", target.name);

    // Нормализуем абсолютные пути, чтобы не зависеть от current_dir() cargo-ndk
    let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    let out_abs: PathBuf = if out.is_absolute() { out.clone() } else { cwd.join(out) };

    // финальная папка и временный вывод cargo-ndk
    let final_dir = out_abs.join("android-arm64");
    std::fs::create_dir_all(&final_dir)
      .map_err(|e| format!("mkdir {}: {e}", final_dir.display()))?;

    let tmp_out = out_abs.join(".ndk-out");
    let _ = std::fs::remove_dir_all(&tmp_out);
    std::fs::create_dir_all(&tmp_out)
      .map_err(|e| format!("mkdir {}: {e}", tmp_out.display()))?;

    // ВАЖНО: --target не указываем; cargo-ndk сам ставит по -t arm64-v8a
    let status = std::process::Command::new("cargo")
      .current_dir(mod_path) // билдим в каталоге мода
      .args([
        "ndk",
        "-t","arm64-v8a",
        "--platform","26",
        "-o", tmp_out.to_str().ok_or("bad tmp_out path")?,
        "build","--release",
      ])
      .status()
      .map_err(|e| format!("cargo-ndk failed to start: {e}"))?;
    if !status.success() {
      return Err("android build failed".into());
    }

    // Ищем любой .so под tmp_out/** (обычно tmp_out/arm64-v8a/lib*.so)
    fn find_first_so(root: &PathBuf) -> Option<PathBuf> {
      let mut stack = vec![root.clone()];
      while let Some(dir) = stack.pop() {
        for ent in std::fs::read_dir(&dir).ok()?.flatten() {
          let p = ent.path();
          if p.is_dir() { stack.push(p); continue; }
          if p.extension().and_then(|e| e.to_str()) == Some("so") {
            return Some(p);
          }
        }
      }
      None
    }

    let so_path = find_first_so(&tmp_out)
      .ok_or_else(|| format!("no .so produced by cargo-ndk under {}", tmp_out.display()))?;

    let file_name = so_path.file_name().and_then(|s| s.to_str()).ok_or("bad output filename")?;
    let dst = final_dir.join(file_name);
    std::fs::copy(&so_path, &dst)
      .map_err(|e| format!("copy {} -> {} failed: {e}", so_path.display(), dst.display()))?;

    // подчистить времянку (не критично, но приятно)
    let _ = std::fs::remove_dir_all(&tmp_out);

    println!("Copied to {}", dst.display());
    return Ok(());
  }

  // --- Все остальные цели как раньше: cargo build/zigbuild + парсинг JSON ---
  println!("Building for {}...", target.name);

  // выбираем subcommand (zigbuild для кросса, если доступен)
  let use_zig = if target.needs_mac {
    has_zigbuild() && has_mac_compiler()
  } else if !is_host_triple(target.triple) {
    has_zigbuild()
  } else {
    false
  };

  let mut cmd = Command::new("cargo");
  cmd.current_dir(mod_path);

  if use_zig {
    cmd.arg("zigbuild");
  } else {
    cmd.arg("build");
  }

  cmd.args([
    "--release",
    "--target",
    target.triple,
    "--message-format=json",
  ]);
  cmd.stdout(Stdio::piped());
  cmd.stderr(Stdio::inherit());

  let mut child = cmd
    .spawn()
    .map_err(|e| format!("Failed to run cargo: {e}"))?;
  let stdout = std::io::BufReader::new(child.stdout.take().unwrap());

  let mut built_files: Vec<PathBuf> = Vec::new();

  for line in stdout.lines() {
    let line = line.map_err(|e| format!("IO error: {e}"))?;
    if line.is_empty() {
      continue;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
      continue;
    };
    if v["reason"] == "compiler-artifact"
      && v["target"]["kind"]
      .as_array()
      .map(|kinds| kinds.iter().any(|k| k == "cdylib"))
      .unwrap_or(false)
    {
      if let Some(arr) = v["filenames"].as_array() {
        for p in arr {
          if let Some(s) = p.as_str() {
            if s.ends_with(&format!(".{}", target.ext)) {
              built_files.push(PathBuf::from(s));
            }
          }
        }
      }
    }
  }

  let status = child
    .wait()
    .map_err(|e| format!("cargo wait failed: {e}"))?;
  if !status.success() {
    return Err(format!("Build failed for {}", target.name));
  }

  if built_files.is_empty() {
    let guess = mod_path.join(format!("target/{}/release", target.triple));
    return Err(format!(
      "Built file not found (no cdylib reported). Checked cargo JSON; try look in: {}",
      guess.display()
    ));
  }

  let chosen = choose_best(&built_files, target)
    .ok_or_else(|| format!("No suitable {} found among: {:?}", target.ext, built_files))?;

  println!("Built {} successfully.", target.name);

  let file_name = chosen.file_name().unwrap().to_string_lossy().into_owned();
  let out_file = add_suffix(&file_name, &format!("-{}", target.name));
  let out_path = out.join(out_file);

  fs::create_dir_all(out).map_err(|e| format!("Failed to create output directory: {e}"))?;
  fs::copy(&chosen, &out_path).map_err(|e| format!("Failed to copy file: {e}"))?;

  println!("Copied to {}", out_path.display());
  Ok(())
}

fn choose_best(files: &[PathBuf], target: &BuildTarget) -> Option<PathBuf> {
  for f in files {
    if f.extension().and_then(|e| e.to_str()) == Some(target.ext)
      && f
      .to_string_lossy()
      .contains(&format!("/target/{}/", target.triple))
    {
      return Some(f.clone());
    }
  }
  files
    .iter()
    .find(|f| f.extension().and_then(|e| e.to_str()) == Some(target.ext))
    .cloned()
}

fn add_suffix(file_name: &str, suffix: &str) -> String {
  if let Some((base, ext)) = file_name.rsplit_once(".") {
    format!("{base}{suffix}.{ext}")
  } else {
    format!("{file_name}{suffix}")
  }
}

/// Простая проверка наличия cargo-ndk
fn has_cargo_ndk() -> bool {
  Command::new("cargo")
    .args(["ndk", "--version"])
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .map(|s| s.success())
    .unwrap_or(false)
}
