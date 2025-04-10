use std::{
    env,
    fmt::Display,
    fs::{self, File},
    io::{Error, Read},
    path::{Path, PathBuf},
    process::exit,
};

use clap::Parser;
use colored::Colorize;
use crc32fast::Hasher;

const CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(required = true, help = "File and directory paths.")]
    paths: Vec<PathBuf>,
    #[arg(short, long, help = "Parse directories recursively.")]
    recursive: bool,
    #[arg(short, long, help = "Output file name.")]
    out_file: Option<PathBuf>,
    #[arg(short, long, help = "Verify a checksum file.")]
    verify: bool,
}

/// Custom error to include a path along with std::io::Error.
struct PathError {
    err: Error,
    path: Option<PathBuf>,
}

impl PathError {
    /// Create a PathError with the provided error and path.
    fn with_path(err: Error, path: &Path) -> PathError {
        PathError {
            err,
            path: Some(path.to_path_buf()),
        }
    }

    /// Create a PathError with just an error.
    fn without_path(err: Error) -> PathError {
        PathError { err, path: None }
    }
}

impl Display for PathError {
    /// Print out both the error and the path.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.path {
            Some(path) => {
                write!(f, "{}: {}", self.err, path.display())
            }
            None => {
                write!(f, "{}", self.err)
            }
        }
    }
}

/// Computes the CRC32 of a file.
///
/// Reads the provided file in chunks of `CHUNK_SIZE` and uses `crc32fast` to compute the CRC32 checksum. In case of
/// any errors it is immediately returned.
fn crc32(file: &Path) -> Result<u32, PathError> {
    let mut fp = File::open(file).map_err(|e| PathError::with_path(e, file))?;
    let mut buf = [0; CHUNK_SIZE];
    let mut hasher = Hasher::new();

    loop {
        let n = fp
            .read(&mut buf)
            .map_err(|e| PathError::with_path(e, file))?;

        if n == 0 {
            break;
        }

        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize())
}

/// Retrieves list of files in a directory.
///
/// If `recursive` is specified, all subdirectories are searched as well. If some error occurs anywhere it is
/// immediately returned.
fn get_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, PathError> {
    let mut files = Vec::new();

    for entry in fs::read_dir(dir).map_err(|e| PathError::with_path(e, dir))? {
        let path = entry.map_err(|e| PathError::with_path(e, dir))?.path();

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
/// If `recursive` is specified, directories are search recusively. Any error is immediately returned.
fn get_all_files(paths: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>, PathError> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.append(&mut get_files(path, recursive)?);
        } else if path.is_file() {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

/// Computes CRC32 values of provided paths and prints them on stdout and optionally writes a output file.
///
/// If `recursive` is specified any directory in `paths` is recursively searched for files. If `out_file` is `None`, no
/// output file is written.
fn create_sfv(
    paths: Vec<PathBuf>,
    recursive: bool,
    out_file: Option<PathBuf>,
) -> Result<(), PathError> {
    let files = get_all_files(&paths, recursive)?;

    let mut out_text = String::from("");
    for file in files {
        let checksum = crc32(&file)?;

        let file = fs::canonicalize(&file).map_err(|e| PathError::with_path(e, &file))?;
        let file = file
            .strip_prefix(env::current_dir().map_err(|e| PathError::without_path(e))?)
            .unwrap_or(&file);

        println!("{} {checksum:08X}", file.display());

        out_text += &format!("{} {checksum:08X}\n", file.display());
    }

    if let Some(path) = out_file {
        fs::write(&path, out_text).map_err(|e| PathError::with_path(e, &path))?;
    }

    Ok(())
}

/// Verify a checksum file.
///
/// Read the checksum file, compute CRC values of the provided files and match them with values in file. Switches
/// current directory to parent directory of SFV file temporarily.
fn verify_sfv(sfv_file: PathBuf) -> Result<bool, PathError> {
    let data = fs::read_to_string(&sfv_file).map_err(|e| PathError::with_path(e, &sfv_file))?;
    let lines = data.lines();

    let cwd = env::current_dir().map_err(|e| PathError::without_path(e))?;
    if let Some(dir) = fs::canonicalize(&sfv_file)
        .map_err(|e| PathError::with_path(e, &sfv_file))?
        .parent()
    {
        env::set_current_dir(dir).map_err(|e| PathError::with_path(e, dir))?;
    }

    let mut all_ok = true;
    for mut line in lines {
        line = line.trim();
        if line.is_empty() || line.starts_with(";") {
            continue;
        }

        let path = line[..line.len() - 8].trim();
        let checksum = line[line.len() - 8..].to_uppercase();

        let computed_checksum = format!("{:08X}", crc32(Path::new(path))?);
        if computed_checksum == checksum {
            println!("{path} {}", "OK".green());
        } else {
            println!(
                "{path} {} {computed_checksum} != {checksum}",
                "FAILED".red()
            );
            all_ok = false;
        }
    }

    env::set_current_dir(&cwd).map_err(|e| PathError::with_path(e, &cwd))?;
    Ok(all_ok)
}

/// Parse command line arguments and call either verify_sfv or create_sfv depending on options provided.
fn main() {
    let mut args = Args::parse();

    if args.verify {
        match verify_sfv(args.paths.remove(0)) {
            Ok(all_ok) => {
                if !all_ok {
                    exit(1);
                }
            }
            Err(e) => {
                println!("An error occured while trying to verify the checksum file...\n{e}");
                exit(1);
            }
        }
    } else {
        if let Err(e) = create_sfv(args.paths, args.recursive, args.out_file) {
            println!("An error occured while trying to compute the checksums...\n{e}");
            exit(1);
        }
    }
}
