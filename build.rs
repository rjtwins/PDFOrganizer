use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PDFIUM_RELEASE: &str = "chromium/7763";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ENV");

    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH")?;
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let asset_name =
        asset_name_for_target(&target_os, &target_arch, &target_env).ok_or_else(|| {
            format!("unsupported PDFium target: {target_arch}-{target_os}-{target_env}")
        })?;
    let library_name = library_name_for_target(&target_os)?;
    let profile_dir = cargo_profile_dir(&PathBuf::from(env::var("OUT_DIR")?))?;
    let cache_dir = profile_dir
        .join("pdfium-cache")
        .join(asset_name.trim_end_matches(".tgz"));
    let cached_library = ensure_cached_library(&cache_dir, asset_name, library_name)?;

    copy_library(&cached_library, &profile_dir.join(library_name))?;
    copy_library(
        &cached_library,
        &profile_dir.join("deps").join(library_name),
    )?;

    Ok(())
}

fn asset_name_for_target(
    target_os: &str,
    target_arch: &str,
    target_env: &str,
) -> Option<&'static str> {
    match (target_os, target_arch, target_env) {
        ("windows", "x86_64", _) => Some("pdfium-win-x64.tgz"),
        ("windows", "x86", _) => Some("pdfium-win-x86.tgz"),
        ("windows", "aarch64", _) => Some("pdfium-win-arm64.tgz"),
        ("linux", "x86_64", "musl") => Some("pdfium-linux-musl-x64.tgz"),
        ("linux", "aarch64", "musl") => Some("pdfium-linux-musl-arm64.tgz"),
        ("linux", "x86_64", _) => Some("pdfium-linux-x64.tgz"),
        ("linux", "x86", _) => Some("pdfium-linux-x86.tgz"),
        ("linux", "aarch64", _) => Some("pdfium-linux-arm64.tgz"),
        ("linux", "arm", _) => Some("pdfium-linux-arm.tgz"),
        ("macos", "x86_64", _) => Some("pdfium-mac-x64.tgz"),
        ("macos", "aarch64", _) => Some("pdfium-mac-arm64.tgz"),
        _ => None,
    }
}

fn library_name_for_target(target_os: &str) -> Result<&'static str, Box<dyn Error>> {
    match target_os {
        "windows" => Ok("pdfium.dll"),
        "linux" => Ok("libpdfium.so"),
        "macos" => Ok("libpdfium.dylib"),
        _ => Err(format!("unsupported PDFium target OS: {target_os}").into()),
    }
}

fn cargo_profile_dir(out_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let mut profile_dir = out_dir.to_path_buf();

    for _ in 0..3 {
        profile_dir = profile_dir
            .parent()
            .ok_or_else(|| format!("unexpected OUT_DIR layout: {}", out_dir.display()))?
            .to_path_buf();
    }

    Ok(profile_dir)
}

fn ensure_cached_library(
    cache_dir: &Path,
    asset_name: &str,
    library_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let cached_library = cache_dir.join(library_name);

    if cached_library.exists() {
        return Ok(cached_library);
    }

    fs::create_dir_all(cache_dir)?;

    let download_url = format!(
        "https://github.com/bblanchon/pdfium-binaries/releases/download/{PDFIUM_RELEASE}/{asset_name}"
    );
    let response = ureq::get(&download_url)
        .set("User-Agent", "PDFOrganizer build script")
        .call()?;
    let decoder = flate2::read::GzDecoder::new(response.into_reader());
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();

        if entry_path.file_name().and_then(|name| name.to_str()) == Some(library_name) {
            let mut output = fs::File::create(&cached_library)?;
            io::copy(&mut entry, &mut output)?;
            return Ok(cached_library);
        }
    }

    Err(format!("could not find {library_name} in {asset_name}").into())
}

fn copy_library(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(source, destination)?;

    Ok(())
}
