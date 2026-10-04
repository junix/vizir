//! Validated staging with best-effort rollback, not a crash-atomic transaction.
//!
//! Ordinary files are replaced by same-directory rename. Existing leaf symlinks
//! keep pointing at their resolved target; multiply linked files are updated in
//! place to preserve their shared identity. All rollback material is prepared
//! before the first destination changes. Concurrent writers/path changes and
//! crashes during publication remain outside this contract.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};

use tempfile::{Builder, TempDir};
use vizir_core::{VizError, VizResult};

pub(crate) struct StagedFile {
    requested: PathBuf,
    destination: PathBuf,
    directory: TempDir,
    staged: PathBuf,
}

impl StagedFile {
    pub(crate) fn new(destination: &Path) -> VizResult<Self> {
        let resolved = crate::paths::destination_for_write(destination)?;
        let create = || -> io::Result<Self> {
            match fs::metadata(&resolved) {
                Ok(metadata) if !metadata.is_file() => {
                    return Err(io::Error::other("destination is not a regular file"));
                }
                Ok(_) => (),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
            let parent = resolved
                .parent()
                .ok_or_else(|| io::Error::other("destination has no parent directory"))?;
            fs::create_dir_all(parent)?;
            let mut builder = Builder::new();
            builder.prefix(".vizir-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                builder.permissions(fs::Permissions::from_mode(0o700));
            }
            let directory = builder.tempdir_in(parent)?;
            // Intentionally absent: a successful no-op renderer cannot reuse
            // either an old destination or a prepopulated temporary artifact.
            let staged = directory.path().join("artifact");
            Ok(Self {
                requested: destination.to_path_buf(),
                destination: resolved,
                directory,
                staged,
            })
        };
        create().map_err(|source| write_error(destination, source))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.staged
    }

    pub(crate) fn write(&self, content: &[u8]) -> VizResult<()> {
        fs::write(&self.staged, content).map_err(|source| write_error(&self.requested, source))
    }
}

enum Previous {
    Absent,
    Replace { backup: PathBuf },
    InPlace { backup: PathBuf, file: File },
}

struct PreparedFile {
    stage: StagedFile,
    previous: Previous,
    touched: bool,
}

impl PreparedFile {
    fn new(stage: StagedFile) -> VizResult<Self> {
        let prepare = || -> io::Result<Previous> {
            if !fs::symlink_metadata(&stage.staged)?.is_file() {
                return Err(io::Error::other("staged artifact is not a regular file"));
            }
            let metadata = match fs::metadata(&stage.destination) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Ok(Previous::Absent);
                }
                Err(error) => return Err(error),
            };
            if !metadata.is_file() {
                return Err(io::Error::other("destination is not a regular file"));
            }
            // A rename must not silently bypass the original file's write
            // permission. Opening without truncation also supports write-only
            // ordinary destinations without reading their old contents.
            let file = OpenOptions::new().write(true).open(&stage.destination)?;
            let backup = stage.directory.path().join("previous");
            if has_multiple_links(&file)? {
                // Updating the shared inode needs a readable independent copy.
                // Reject an unreadable backup before mutating any destination.
                let mut source = File::open(&stage.destination)?;
                let mut saved = File::create(&backup)?;
                io::copy(&mut source, &mut saved)?;
                saved.flush()?;
                Ok(Previous::InPlace { backup, file })
            } else {
                fs::set_permissions(&stage.staged, metadata.permissions())?;
                // Linking preserves write-only files and original inode/mode
                // on rollback. Filesystems without hard links may use a copy.
                if fs::hard_link(&stage.destination, &backup).is_err() {
                    fs::copy(&stage.destination, &backup)?;
                }
                Ok(Previous::Replace { backup })
            }
        };
        let previous = prepare().map_err(|source| write_error(&stage.requested, source))?;
        Ok(Self {
            stage,
            previous,
            touched: false,
        })
    }

    fn publish(&mut self) -> io::Result<()> {
        match &mut self.previous {
            Previous::InPlace { file, .. } => {
                let mut source = File::open(&self.stage.staged)?;
                self.touched = true;
                overwrite(file, &mut source)
            }
            Previous::Absent | Previous::Replace { .. } => {
                fs::rename(&self.stage.staged, &self.stage.destination)?;
                self.touched = true;
                Ok(())
            }
        }
    }

    fn rollback(&mut self) -> io::Result<()> {
        if !self.touched {
            return Ok(());
        }
        match &mut self.previous {
            Previous::Absent => fs::remove_file(&self.stage.destination),
            Previous::Replace { backup } => fs::rename(backup, &self.stage.destination),
            Previous::InPlace { backup, file } => overwrite(file, &mut File::open(backup)?),
        }
    }
}

fn overwrite(destination: &mut File, source: &mut File) -> io::Result<()> {
    destination.rewind()?;
    destination.set_len(0)?;
    io::copy(source, destination)?;
    destination.flush()
}

struct Publication {
    files: Vec<PreparedFile>,
}

impl Publication {
    fn prepare(files: Vec<StagedFile>) -> VizResult<Self> {
        Ok(Self {
            files: files
                .into_iter()
                .map(PreparedFile::new)
                .collect::<VizResult<_>>()?,
        })
    }

    fn commit(mut self) -> VizResult<()> {
        for index in 0..self.files.len() {
            if let Err(source) = self.files[index].publish() {
                let error = write_error(&self.files[index].stage.requested, source);
                let mut failures = Vec::new();
                for file in self.files[..=index].iter_mut().rev() {
                    if let Err(error) = file.rollback() {
                        failures.push(format!("{}: {error}", file.stage.requested.display()));
                    }
                }
                if failures.is_empty() {
                    return Err(error);
                }
                // Never discard the only saved copy when rollback itself
                // fails. Leave recovery data and diagnose its exact location.
                let recovery = self
                    .files
                    .into_iter()
                    .map(|file| file.stage.directory.keep().display().to_string())
                    .collect::<Vec<_>>();
                return Err(VizError::Diagnostic(format!(
                    "{error}; VIZ-OUTPUT-0001: rollback failed for {}; recovery files retained in {}",
                    failures.join("; "),
                    recovery.join(", ")
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn publish(files: Vec<StagedFile>) -> VizResult<()> {
    Publication::prepare(files)?.commit()
}

fn write_error(path: &Path, source: io::Error) -> VizError {
    VizError::Write {
        path: path.display().to_string(),
        source,
    }
}

#[cfg(unix)]
fn has_multiple_links(file: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(file.metadata()?.nlink() > 1)
}

#[cfg(windows)]
fn has_multiple_links(file: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the File keeps a valid handle alive and info has sufficient space
    // for the documented output structure, read only after a successful call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.assume_init() }.nNumberOfLinks > 1)
}

#[cfg(not(any(unix, windows)))]
fn has_multiple_links(_file: &File) -> io::Result<bool> {
    // Preserve shared identity conservatively on platforms without a link
    // count API, requiring a readable backup before any in-place update.
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(path: &Path, bytes: &[u8]) -> StagedFile {
        let stage = StagedFile::new(path).unwrap();
        stage.write(bytes).unwrap();
        stage
    }

    fn assert_no_staging(directory: &Path) {
        for entry in fs::read_dir(directory).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vizir-")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn staging_directories_are_private_without_changing_the_parent_mode() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o750)).unwrap();
        let stage = StagedFile::new(&temporary.path().join("plot")).unwrap();
        assert_eq!(
            fs::metadata(stage.directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(temporary.path()).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert!(!stage.path().exists());
    }
    #[test]
    fn failed_second_publication_restores_existing_or_removes_new_first_output() {
        for existing in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let first = temporary.path().join("plot");
            let second = temporary.path().join("manifest");
            if existing {
                fs::write(&first, b"old plot").unwrap();
            }
            fs::write(&second, b"old manifest").unwrap();
            let publication = Publication::prepare(vec![
                stage(&first, b"new plot"),
                stage(&second, b"new manifest"),
            ])
            .unwrap();
            // Deterministically fail the second rename after all backups exist.
            fs::remove_file(&publication.files[1].stage.staged).unwrap();
            assert!(publication.commit().is_err());
            if existing {
                assert_eq!(fs::read(first).unwrap(), b"old plot");
            } else {
                assert!(!first.exists());
            }
            assert_eq!(fs::read(second).unwrap(), b"old manifest");
            assert_no_staging(temporary.path());
        }
    }

    #[test]
    fn failed_backup_preparation_leaves_every_destination_unchanged() {
        let temporary = tempfile::tempdir().unwrap();
        let first = temporary.path().join("plot");
        let second = temporary.path().join("manifest");
        fs::write(&first, b"old plot").unwrap();
        let stages = vec![stage(&first, b"new plot"), stage(&second, b"new manifest")];
        fs::create_dir(&second).unwrap();
        assert!(publish(stages).is_err());
        assert_eq!(fs::read(first).unwrap(), b"old plot");
        assert!(second.is_dir());
        assert_no_staging(temporary.path());
    }

    #[test]
    fn rollback_failure_retains_recovery_directory_and_reports_it() {
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("plot");
        fs::write(&destination, b"old plot").unwrap();
        let mut publication = Publication::prepare(vec![stage(&destination, b"new plot")]).unwrap();
        publication.files[0].publish().unwrap();
        let recovery = publication.files[0].stage.directory.path().to_path_buf();
        // Make the required restore impossible, without runtime fault hooks.
        fs::create_dir(recovery.join("blocked")).unwrap();
        fs::rename(&destination, recovery.join("new plot")).unwrap();
        fs::create_dir(&destination).unwrap();
        let error = publication.commit().unwrap_err().to_string();
        assert!(error.contains("VIZ-OUTPUT-0001"), "{error}");
        assert!(error.contains(&recovery.display().to_string()), "{error}");
        assert_eq!(fs::read(recovery.join("previous")).unwrap(), b"old plot");
    }

    #[cfg(unix)]
    #[test]
    fn failed_second_publication_rolls_back_a_hardlinked_first_output_in_place() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let temporary = tempfile::tempdir().unwrap();
        let first = temporary.path().join("plot");
        let alias = temporary.path().join("alias");
        let second = temporary.path().join("manifest");
        fs::write(&first, b"old plot").unwrap();
        fs::set_permissions(&first, fs::Permissions::from_mode(0o640)).unwrap();
        fs::hard_link(&first, &alias).unwrap();
        let inode = fs::metadata(&first).unwrap().ino();
        let publication = Publication::prepare(vec![
            stage(&first, b"new plot with a different length"),
            stage(&second, b"new manifest"),
        ])
        .unwrap();
        fs::remove_file(&publication.files[1].stage.staged).unwrap();
        assert!(publication.commit().is_err());
        for path in [&first, &alias] {
            assert_eq!(fs::read(path).unwrap(), b"old plot");
            let metadata = fs::metadata(path).unwrap();
            assert_eq!(metadata.ino(), inode);
            assert_eq!(metadata.mode() & 0o777, 0o640);
            assert_eq!(metadata.nlink(), 2);
        }
        assert!(!second.exists());
        assert_no_staging(temporary.path());
    }
}
