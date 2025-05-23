//! Computes the CRC32 checksum of files provided.
//!
//! Can also verify SFV and create SFV files.
use std::{
    env,
    fmt::Write,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{Context, Error, Result};
use clap::Parser;
use colored::Colorize;
use crc32fast::Hasher;

/// Number of bytes to read at once.
const CHUNK_SIZE: usize = 1024 * 1024;

/// Command line arguments.
#[derive(Parser)]
#[command(version, about = None, long_about = None)]
struct Args {
    #[arg(required = true, help = "File and directory paths")]
    paths: Vec<PathBuf>,
    #[arg(short, long, help = "Parse directories recursively")]
    recursive: bool,
    #[arg(short, long, help = "Output file name")]
    out_file: Option<PathBuf>,
    #[arg(short, long, help = "Verify a checksum file")]
    verify: bool,
}

/// Computes the CRC32 of a file.
///
/// Reads the provided file in chunks of `CHUNK_SIZE` and uses `crc32fast` to compute the CRC32 checksum. Any error is
/// propagated with added context.
fn crc32<P>(file: P) -> Result<u32>
where
    P: AsRef<Path>,
{
    let file = file.as_ref();
    let mut fp =
        File::open(file).with_context(|| format!("Failed to open file {}", file.display()))?;
    let mut buf = vec![0; CHUNK_SIZE];
    let mut hasher = Hasher::new();

    loop {
        let n = fp
            .read(&mut buf)
            .with_context(|| format!("Error while reading file {}", file.display()))?;

        if n == 0 {
            break;
        }

        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize())
}

/// Retrieves list of files in a directory.
///
/// If `recursive` is specified, all subdirectories are searched as well. Errors are propagated with added context.
fn get_files<P>(dir: P, recursive: bool) -> Result<Vec<PathBuf>>
where
    P: AsRef<Path>,
{
    let dir = dir.as_ref();
    let mut files = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("Failed to read directory {}", dir.display()))?
    {
        let path = entry
            .with_context(|| format!("Error while reading directory {}", dir.display()))?
            .path();

        if recursive && path.is_dir() {
            files.append(&mut get_files(&path, true)?);
        } else if path.is_file() {
            files.push(path);
        }
    }

    Ok(files)
}

/// Returns a sorted list of all files in given paths.
///
/// If `recursive` is specified, directories are search recusively. Any error is propagated.
fn get_all_files<A>(paths: A, recursive: bool) -> Result<Vec<PathBuf>>
where
    A: IntoIterator<Item = PathBuf>,
{
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.append(&mut get_files(&path, recursive)?);
        } else if path.is_file() {
            files.push(path);
        } else {
            return Err(Error::msg(format!(
                "{} is neither a file nor a directory",
                path.display()
            )));
        }
    }

    files.sort();
    Ok(files)
}

/// Computes CRC32 values of provided paths and prints them on stdout and optionally writes a output file.
///
/// If `recursive` is specified any directory in `paths` is recursively searched for files. If `out_file` is `None`, no
/// output file is written.
fn create_sfv<A>(paths: A, recursive: bool, out_file: Option<PathBuf>) -> Result<()>
where
    A: IntoIterator<Item = PathBuf>,
{
    let files = get_all_files(paths, recursive)?;

    let mut out_text = String::default();
    for file in files {
        let checksum = crc32(&file)?;
        let cwd = env::current_dir().context("Failed to get current directory")?;
        let cwd = fs::canonicalize(&cwd)
            .with_context(|| format!("Failed to get canonical path for {}", cwd.display()))?;
        let file_canonical = fs::canonicalize(&file)
            .with_context(|| format!("Failed to get canonical path for {}", file.display()))?;
        let file = file_canonical.strip_prefix(cwd).unwrap_or(&file);

        println!("{} {checksum:08X}", file.display());

        writeln!(out_text, "{} {checksum:08X}", file.display())
            .context("Failed to write to string")?;
    }

    if let Some(path) = out_file {
        fs::write(&path, out_text)
            .with_context(|| format!("Failed to write to {}", path.display()))?;
    }

    Ok(())
}

/// Verify a checksum file.
///
/// Read the checksum file, compute CRC values of the provided files and match them with values in file. Switches
/// current directory to parent directory of SFV file temporarily.
fn verify_sfv<P>(sfv_file: P) -> Result<()>
where
    P: Into<PathBuf>,
{
    let sfv_file = sfv_file.into();
    let data = fs::read_to_string(&sfv_file)
        .with_context(|| format!("Failed to read file {}", sfv_file.display()))?;
    let lines = data.lines();

    let cwd = env::current_dir().context("Failed to get current directory")?;
    if let Some(dir) = fs::canonicalize(&sfv_file)
        .with_context(|| format!("Failed to get canonical path for {}", sfv_file.display()))?
        .parent()
    {
        env::set_current_dir(dir)
            .with_context(|| format!("Failed to set current directory to {}", dir.display()))?;
    }

    for mut line in lines {
        line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        let path = line[..line.len() - 8].trim();
        let checksum = line[line.len() - 8..].to_uppercase();

        match crc32(path) {
            Ok(computed_checksum) => {
                let computed_checksum = format!("{computed_checksum:08X}");
                if computed_checksum == checksum {
                    println!("{path} {}", "OK".green().bold());
                } else {
                    println!(
                        "{path} {} {computed_checksum} ≠ {checksum}",
                        "FAIL".yellow().bold()
                    );
                }
            }
            Err(e) => {
                println!("{path} {} {e:#}", "ERROR".red().bold());
            }
        }
    }

    env::set_current_dir(&cwd)
        .with_context(|| format!("Failed to set current directory to {}", cwd.display()))?;
    Ok(())
}

/// Parse command line arguments and call either `verify_sfv` or `create_sfv` depending on options provided.
fn main() -> ExitCode {
    let mut args = Args::parse();

    let mut exit_code = ExitCode::SUCCESS;
    if args.verify {
        if let Err(e) = verify_sfv(args.paths.remove(0)) {
            println!("{} {e:#}", "[ERROR]".red().bold());
            exit_code = ExitCode::FAILURE;
        }
    } else if let Err(e) = create_sfv(args.paths, args.recursive, args.out_file) {
        println!("{} {e:#}", "[ERROR]".red().bold());
        exit_code = ExitCode::FAILURE;
    }

    exit_code
}
