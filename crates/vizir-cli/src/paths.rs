use std::fs::{self, Metadata};
use std::io;
use std::path::{Component, Path, PathBuf};

use vizir_core::{VizError, VizResult};

struct CheckedPath<'a> {
    role: &'static str,
    original: &'a Path,
    resolved: PathBuf,
    metadata: Option<Metadata>,
}

impl<'a> CheckedPath<'a> {
    fn new(role: &'static str, path: &'a Path) -> VizResult<Self> {
        let inspect = || -> io::Result<Self> {
            // Inspect the original spelling first. Cleaning symlink/.. before
            // consulting the filesystem can point at a different file.
            let metadata = metadata_if_present(path)?;
            let resolved = resolve_destination(path, &mut 64)?;
            let metadata = match metadata {
                Some(metadata) => Some(metadata),
                // create_dir_all can make missing/../existing writable. Check
                // that projected file's identity as well, including hard links.
                None => metadata_if_present(&resolved)?,
            };
            Ok(Self {
                role,
                original: path,
                resolved,
                metadata,
            })
        };
        inspect().map_err(|error| path_error(role, path, error))
    }
}

/// A read-only preflight against accidental overwrites, not a race-free write
/// transaction. Publication resolves the same destination semantics while
/// keeping the caller's original paths for reports and diagnostics.
pub(crate) fn check_destinations(
    input: &Path,
    output: Option<&Path>,
    manifest: Option<&Path>,
) -> VizResult<()> {
    if output.is_none() && manifest.is_none() {
        return Ok(());
    }
    let mut paths = vec![CheckedPath::new("input", input)?];
    for (role, path) in [("output", output), ("manifest", manifest)] {
        if let Some(path) = path {
            paths.push(CheckedPath::new(role, path)?);
        }
    }
    for (index, destination) in paths.iter().enumerate().skip(1) {
        for previous in &paths[..index] {
            if destination.resolved == previous.resolved
                || same_file(destination, previous)
                    .map_err(|error| path_error(destination.role, destination.original, error))?
            {
                return Err(VizError::Diagnostic(format!(
                    "VIZ-PATH-0001: {} path {} refers to the same file as {} path {}; choose distinct input, output, and manifest paths",
                    destination.role,
                    destination.original.display(),
                    previous.role,
                    previous.original.display(),
                )));
            }
        }
    }
    Ok(())
}

/// Both are read-only inputs, but one path cannot claim both source contracts.
pub(crate) fn check_distinct_sources(input: &Path, links: &Path) -> VizResult<()> {
    let input = CheckedPath::new("input", input)?;
    let links = CheckedPath::new("selection links", links)?;
    if input.resolved == links.resolved
        || same_file(&input, &links).map_err(|e| path_error(links.role, links.original, e))?
    {
        return Err(VizError::Diagnostic("VIZ-PATH-0001: input and selection links paths refer to the same file; choose distinct sources".into()));
    }
    Ok(())
}

fn path_error(role: &str, path: &Path, error: io::Error) -> VizError {
    VizError::Diagnostic(format!(
        "VIZ-PATH-0002: cannot check {role} path {}: {error}",
        path.display()
    ))
}

fn metadata_if_present(path: &Path) -> io::Result<Option<Metadata>> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn same_file(left: &CheckedPath<'_>, right: &CheckedPath<'_>) -> io::Result<bool> {
    let (Some(left_metadata), Some(right_metadata)) = (&left.metadata, &right.metadata) else {
        return Ok(false);
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(left_metadata.dev() == right_metadata.dev()
            && left_metadata.ino() == right_metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = (left_metadata, right_metadata);
        same_file::is_same_file(&left.resolved, &right.resolved)
    }
}

pub(crate) fn destination_for_write(path: &Path) -> VizResult<PathBuf> {
    resolve_destination(path, &mut 64).map_err(|error| path_error("output", path, error))
}

// Resolve each existing component before consuming `..`; allow missing parents
// without creating them. Rechecking each component also catches symlinks reached
// after a missing/.. pair and dangling links to a not-yet-created destination.
fn resolve_destination(path: &Path, links_left: &mut usize) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => resolved.push(component),
            Component::CurDir => (),
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(_) => {
                resolved.push(component);
                match fs::canonicalize(&resolved) {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        match fs::symlink_metadata(&resolved) {
                            Ok(metadata) if metadata.file_type().is_symlink() => {
                                if *links_left == 0 {
                                    return Err(io::Error::other("too many symbolic links"));
                                }
                                *links_left -= 1;
                                let target = fs::read_link(&resolved)?;
                                resolved.pop();
                                resolved = resolve_destination(&resolved.join(target), links_left)?;
                            }
                            Ok(_) => (),
                            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                            Err(error) => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}
