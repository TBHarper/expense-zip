//! expense-zip: zips the files in the current directory into
//! `<LAST_NAME><FIRST_NAME>Expenses<YYYY><MM>.zip`, written to the current directory.
//!
//! - YYYY is the parent directory's name, e.g. `2026`.
//! - MM is the first two characters of the current directory's name, e.g. `03` from `03 March`.
//! - FIRST_NAME and LAST_NAME are baked in at compile time from environment variables:
//!   `$env:FIRST_NAME = "Jane"; $env:LAST_NAME = "Doe"; cargo build --release`
//! - Uses 7-Zip if one of `SEVEN_ZIP_CANDIDATES` runs, otherwise the built-in `zip` crate.
//! - An existing archive with the same name is replaced.

use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const FIRST_NAME: &str = env!(
    "FIRST_NAME",
    r#"set FIRST_NAME and LAST_NAME when building, e.g. $env:FIRST_NAME="Jane"; $env:LAST_NAME="Doe"; cargo build --release"#
);
const LAST_NAME: &str = env!(
    "LAST_NAME",
    r#"set FIRST_NAME and LAST_NAME when building, e.g. $env:FIRST_NAME="Jane"; $env:LAST_NAME="Doe"; cargo build --release"#
);

/// 7-Zip executables to try, in order: `7z` on PATH, then the default install locations.
const SEVEN_ZIP_CANDIDATES: [&str; 3] = [
    "7z",
    r"C:\Program Files\7-Zip\7z.exe",
    r"C:\Program Files (x86)\7-Zip\7z.exe",
];

fn usage() -> String {
    format!(
        "\
Usage: expense-zip [-r] [-h] [-V]

Zips the files in the current directory into
{LAST_NAME}{FIRST_NAME}Expenses<YYYY><MM>.zip in the current directory, where
YYYY is the parent directory's name and MM is the first two characters of the
current directory's name. Uses 7-Zip if installed, otherwise a built-in zip
writer. An existing archive with that name is replaced.

Options:
  -r, --recurse   Also include files in subdirectories (default: top level only)
  -h, --help      Print this help
  -V, --version   Print version
"
    )
}

fn main() -> ExitCode {
    let mut recurse = false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "-r" | "--recurse" => recurse = true,
            "-h" | "--help" => {
                print!("{}", usage());
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("expense-zip {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other => {
                eprint!("error: unknown argument '{other}'\n\n{}", usage());
                return ExitCode::from(2);
            }
        }
    }

    // Our own executable, so it isn't zipped if it lives in the folder being zipped.
    let own_exe = env::current_exe().and_then(fs::canonicalize).ok();
    let result = env::current_dir()
        .map_err(Into::into)
        .and_then(|dir| run(&dir, own_exe.as_deref(), recurse));

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(dir: &Path, own_exe: Option<&Path>, recurse: bool) -> Result<(), Box<dyn Error>> {
    let zip_name = archive_name(dir)?;
    let zip_path = dir.join(&zip_name);

    // Replace any archive left by a previous run (and keep it out of the file list).
    if zip_path.exists() {
        fs::remove_file(&zip_path)?;
    }

    let mut files = Vec::new();
    collect_files(dir, Path::new(""), recurse, &mut files)?;
    files.retain(|file| fs::canonicalize(dir.join(file)).ok().as_deref() != own_exe);
    files.sort();
    if files.is_empty() {
        return Err(format!("no files to zip in {}", dir.display()).into());
    }

    let method = match zip_with_7zip(dir, &zip_name, &files)? {
        Some(seven_zip) => format!("7-Zip: {seven_zip}"),
        None => {
            zip_with_builtin(dir, &zip_path, &files)?;
            "built-in zip, 7-Zip not found".to_string()
        }
    };
    println!("Created {zip_name} with {} files ({method})", files.len());
    Ok(())
}

/// `<LAST><FIRST>Expenses<YYYY><MM>.zip`, where YYYY is the name of `dir`'s
/// parent and MM is the first two characters of `dir`'s name.
fn archive_name(dir: &Path) -> Result<String, Box<dyn Error>> {
    let month: String = dir_name(dir)?.chars().take(2).collect();
    let parent = dir.parent().ok_or("the current directory has no parent")?;
    let year = dir_name(parent)?;
    Ok(format!("{LAST_NAME}{FIRST_NAME}Expenses{year}{month}.zip"))
}

fn dir_name(dir: &Path) -> Result<&str, Box<dyn Error>> {
    dir.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("can't get a directory name from {}", dir.display()).into())
}

/// Appends the files in `dir/rel` to `files` as paths relative to `dir`.
/// Descends into subdirectories only if `recurse` is true.
fn collect_files(
    dir: &Path,
    rel: &Path,
    recurse: bool,
    files: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir.join(rel))? {
        let rel_path = rel.join(entry?.file_name());
        if dir.join(&rel_path).is_dir() {
            if recurse {
                collect_files(dir, &rel_path, recurse, files)?;
            }
        } else {
            files.push(rel_path);
        }
    }
    Ok(())
}

/// Zips `files` (relative to `dir`) into `dir/zip_name` with 7-Zip.
/// Returns the 7-Zip executable used, or `None` if none of the candidates exist.
fn zip_with_7zip(
    dir: &Path,
    zip_name: &str,
    files: &[PathBuf],
) -> Result<Option<&'static str>, Box<dyn Error>> {
    for seven_zip in SEVEN_ZIP_CANDIDATES {
        let status = Command::new(seven_zip)
            .current_dir(dir)
            // a = add to archive, -tzip = zip format, -mx=9 = max compression,
            // -bso0 -bsp0 = no normal output or progress (errors still print),
            // -- = end of switches, so file names starting with '-' are safe.
            .args(["a", "-tzip", "-mx=9", "-bso0", "-bsp0", "--", zip_name])
            .args(files)
            .status();
        match status {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("couldn't run {seven_zip}: {e}").into()),
            Ok(status) if status.success() => return Ok(Some(seven_zip)),
            Ok(status) => return Err(format!("{seven_zip} failed ({status})").into()),
        }
    }
    Ok(None)
}

/// Zips `files` (relative to `dir`) into `zip_path` with the `zip` crate.
/// Entries are stamped with the current time (UTC), not the files' own times.
fn zip_with_builtin(dir: &Path, zip_path: &Path, files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9));
    let mut zip = ZipWriter::new(File::create(zip_path)?);
    for file in files {
        // Zip entry names always use '/' as the separator.
        let entry_name = file
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        zip.start_file(entry_name, options)?;
        io::copy(&mut File::open(dir.join(file))?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_name_uses_parent_year_and_month_prefix() {
        let dir = Path::new("expenses").join("2026").join("03 March");
        assert_eq!(
            archive_name(&dir).unwrap(),
            format!("{LAST_NAME}{FIRST_NAME}Expenses202603.zip")
        );
    }

    #[test]
    fn builtin_zip_round_trips_top_level_and_recursive_files() {
        let dir = env::temp_dir().join(format!("expense-zip-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "top").unwrap();
        fs::write(dir.join("sub").join("b.txt"), "nested").unwrap();

        let mut top_level = Vec::new();
        collect_files(&dir, Path::new(""), false, &mut top_level).unwrap();
        assert_eq!(top_level, [PathBuf::from("a.txt")]);

        let mut all = Vec::new();
        collect_files(&dir, Path::new(""), true, &mut all).unwrap();
        all.sort();
        assert_eq!(
            all,
            [PathBuf::from("a.txt"), Path::new("sub").join("b.txt")]
        );

        let zip_path = dir.join("out.zip");
        zip_with_builtin(&dir, &zip_path, &all).unwrap();

        let mut archive = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.txt", "sub/b.txt"]);
        let nested = io::read_to_string(archive.by_name("sub/b.txt").unwrap()).unwrap();
        assert_eq!(nested, "nested");

        drop(archive);
        fs::remove_dir_all(&dir).unwrap();
    }
}
