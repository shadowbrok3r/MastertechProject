//! Build and inspect a real FAT ESP image.
//!
//! QEMU's virtual-FAT (`fat:rw:`) aborts on directory creation and mangles the
//! host copy of anything the guest writes, so firmware that writes files cannot
//! be tested against it. This produces a genuine FAT volume the firmware can
//! write to, and reads the result back out afterwards.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fatfs::{FileSystem, FormatVolumeOptions, FsOptions, format_volume};
use fscommon::BufStream;

#[derive(Parser)]
#[command(name = "esp-image", about = "Build and inspect FAT ESP images")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Format a fresh image and copy a directory tree into it.
    Build {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Image size in MiB; FAT32 needs at least 64.
        #[arg(long, default_value_t = 256)]
        size_mb: u64,
    },
    /// List a directory inside an image.
    List {
        #[arg(long)]
        image: PathBuf,
        #[arg(long, default_value = "/")]
        dir: String,
    },
    /// Print a file from inside an image.
    Cat {
        #[arg(long)]
        image: PathBuf,
        #[arg(long)]
        path: String,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Build { from, out, size_mb } => build(&from, &out, size_mb),
        Cmd::List { image, dir } => list(&image, &dir),
        Cmd::Cat { image, path } => cat(&image, &path),
    }
}

fn open_image(path: &Path, write: bool) -> Result<BufStream<std::fs::File>> {
    let f = OpenOptions::new()
        .read(true)
        .write(write)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    Ok(BufStream::new(f))
}

fn build(from: &Path, out: &Path, size_mb: u64) -> Result<()> {
    if !from.is_dir() {
        bail!("{} is not a directory", from.display());
    }
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(out)
        .with_context(|| format!("create {}", out.display()))?;
    f.set_len(size_mb * 1024 * 1024)?;
    let mut stream = BufStream::new(f);
    format_volume(&mut stream, FormatVolumeOptions::new()).context("format FAT volume")?;

    let mut files = 0usize;
    let mut bytes = 0u64;
    {
        let fs = FileSystem::new(&mut stream, FsOptions::new()).context("mount fresh volume")?;
        copy_dir(&fs.root_dir(), from, "", &mut files, &mut bytes)?;
        fs.unmount().context("unmount")?;
    }
    stream.flush()?;

    println!(
        "wrote {} ({} MiB): {files} files, {:.1} MiB of content",
        out.display(),
        size_mb,
        bytes as f64 / 1024.0 / 1024.0
    );
    Ok(())
}

/// Recursively copy `host` into the FAT directory `dir`.
fn copy_dir<T: fatfs::ReadWriteSeek>(
    dir: &fatfs::Dir<'_, T>,
    host: &Path,
    prefix: &str,
    files: &mut usize,
    bytes: &mut u64,
) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(host)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if e.file_type()?.is_dir() {
            let sub = dir
                .create_dir(&name)
                .with_context(|| format!("create dir {prefix}/{name}"))?;
            copy_dir(&sub, &path, &format!("{prefix}/{name}"), files, bytes)?;
        } else {
            let mut src = std::fs::File::open(&path)?;
            let mut buf = Vec::new();
            src.read_to_end(&mut buf)?;
            let mut dst = dir
                .create_file(&name)
                .with_context(|| format!("create file {prefix}/{name}"))?;
            dst.truncate()?;
            dst.write_all(&buf)
                .with_context(|| format!("write {prefix}/{name}"))?;
            *files += 1;
            *bytes += buf.len() as u64;
        }
    }
    Ok(())
}

/// Walk a backslash- or slash-separated path to its directory.
fn open_dir<'a, T: fatfs::ReadWriteSeek>(
    fs: &'a FileSystem<T>,
    path: &str,
) -> Result<fatfs::Dir<'a, T>> {
    let mut dir = fs.root_dir();
    for part in path.replace('\\', "/").split('/').filter(|p| !p.is_empty()) {
        dir = dir
            .open_dir(part)
            .with_context(|| format!("open dir {part}"))?;
    }
    Ok(dir)
}

fn list(image: &Path, path: &str) -> Result<()> {
    let mut stream = open_image(image, false)?;
    let fs = FileSystem::new(&mut stream, FsOptions::new())?;
    let dir = open_dir(&fs, path)?;
    println!("{path}");
    for e in dir.iter() {
        let e = e?;
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        if e.is_dir() {
            println!("  <DIR>  {name}");
        } else {
            println!("  {:>9}  {name}", e.len());
        }
    }
    Ok(())
}

fn cat(image: &Path, path: &str) -> Result<()> {
    let mut stream = open_image(image, false)?;
    let fs = FileSystem::new(&mut stream, FsOptions::new())?;
    let norm = path.replace('\\', "/");
    let (dir_part, file_part) = match norm.rsplit_once('/') {
        Some((d, f)) => (d.to_string(), f.to_string()),
        None => (String::new(), norm.clone()),
    };
    let dir = open_dir(&fs, &dir_part)?;
    let mut f = dir
        .open_file(&file_part)
        .with_context(|| format!("open {path}"))?;
    f.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    print!("{}", String::from_utf8_lossy(&buf));
    Ok(())
}
