use std::{
    env, fs, io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

const BASE_URL: &str = "https://github.com/2278535805/prpr-avc-ffmpeg/releases/download";
const FFMPEG_LIBS: &[&str] = &["libavcodec.a", "libavformat.a", "libavutil.a", "libswresample.a", "libswscale.a"];

fn main() {
    if let Err(err) = run() {
        panic!("[prpr-avc] {err}");
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let version_file = manifest_dir.join("ffmpeg-version");
    let version = fs::read_to_string(&version_file)?.trim().to_owned();
    if version.is_empty() {
        return Err("ffmpeg-version is empty".into());
    }

    let target = env::var("TARGET")?;
    let is_msvc = env::var("CARGO_CFG_TARGET_ENV").map_or(false, |env| env == "msvc");
    let libs_dir = env::var_os("PRPR_AVC_LIBS")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("static-lib"));
    let target_dir = libs_dir.join(&target);

    let x264_lib = if is_msvc { "libx264.lib" } else { "libx264.a" };
    let expected_libs = std::iter::once(x264_lib).chain(FFMPEG_LIBS.iter().copied()).collect::<Vec<_>>();

    println!("cargo:rerun-if-changed={}", version_file.display());
    println!("cargo:rerun-if-env-changed=PRPR_AVC_LIBS");

    ensure_static_lib(&libs_dir, &target, &version, &expected_libs)?;

    println!("cargo:rustc-link-search={}", target_dir.display());
    println!("cargo:rustc-link-lib=static={}", if is_msvc { "libx264" } else { "x264" });
    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!("cargo:rustc-link-lib=bcrypt");
    } else {
        println!("cargo:rustc-link-lib=z");
    }
    println!("cargo:rerun-if-changed={}", target_dir.display());
    Ok(())
}

fn ensure_static_lib(libs_dir: &Path, target: &str, version: &str, expected_libs: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let target_dir = libs_dir.join(target);
    if cache_is_valid(&target_dir, version, expected_libs) {
        return Ok(());
    }

    fs::create_dir_all(&target_dir)?;

    let url = format!("{BASE_URL}/{version}/{target}.tar.gz");
    println!("cargo:warning=[prpr-avc] Downloading FFmpeg static libraries for {target} ({version})");

    download_and_extract(&url, &target_dir)?;
    validate_libs(&target_dir, expected_libs)?;
    fs::write(target_dir.join(".version"), format!("{version}\n"))?;

    Ok(())
}

fn cache_is_valid(path: &Path, version: &str, expected_libs: &[&str]) -> bool {
    let Ok(actual_version) = fs::read_to_string(path.join(".version")) else {
        return false;
    };
    actual_version.trim() == version && expected_libs.iter().all(|name| path.join(name).is_file())
}

fn validate_libs(path: &Path, expected_libs: &[&str]) -> io::Result<()> {
    for name in expected_libs {
        if !path.join(name).is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("downloaded archive is missing {name}")));
        }
    }
    Ok(())
}

fn download_and_extract(url: &str, output_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(120))
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "prpr-avc-build-script")
        .call()
        .map_err(|err| format!("failed to download {url}: {err}"))?;
    let decoder = flate2::read::GzDecoder::new(response.into_reader());
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if !safe_archive_path(&path) {
            return Err(format!("archive contains an unsafe path: {}", path.display()).into());
        }
        if !entry.header().entry_type().is_file() {
            return Err(format!("archive contains a non-file entry: {}", path.display()).into());
        }
        entry.unpack(output_dir.join(path))?;
    }
    Ok(())
}

fn safe_archive_path(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|component| matches!(component, Component::Normal(_)))
}
