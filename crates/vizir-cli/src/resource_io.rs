//! Shared bounded explicit regular-file input; no discovery or implicit reads.
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;
use vizir_core::{VizError, VizResult};

pub(crate) fn read_bounded_regular_with_prefix(
    path: &Path,
    limit: usize,
    role: &str,
    prefix: &str,
) -> VizResult<Vec<u8>> {
    let read_error = |source| VizError::Read {
        path: path.display().to_string(),
        source,
    };
    let too_large = || {
        if prefix == "VIZ-PROVIDER" {
            return VizError::Diagnostic(format!(
                "VIZ-PROVIDER-0004: {role} exceeds the {limit} byte limit"
            ));
        }
        if prefix == "VIZ-CSV" {
            return VizError::Diagnostic(format!(
                "VIZ-CSV-0010: {role} exceeds the {limit} byte limit"
            ));
        }
        let kind = if role == "font resource" {
            "byte"
        } else {
            "parsing"
        };
        VizError::Diagnostic(format!(
            "{prefix}-0004: {role} exceeds the {} MiB {kind} limit",
            limit / (1024 * 1024)
        ))
    };
    let file = open_regular(path, role, prefix)?;
    if file.metadata().map_err(read_error)?.len() > limit as u64 {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

fn open_regular(path: &Path, role: &str, prefix: &str) -> VizResult<File> {
    let read_error = |source| VizError::Read {
        path: path.display().to_string(),
        source,
    };
    let nonregular = || {
        let code = if prefix == "VIZ-CSV" {
            "VIZ-CSV-0104".to_owned()
        } else {
            format!("{prefix}-0008")
        };
        VizError::Diagnostic(format!("{code}: {role} must be a regular file"))
    };
    // Canonicalize the explicit operator mapping, including any symlink target.
    let resolved = std::fs::canonicalize(path).map_err(read_error)?;
    if !std::fs::metadata(&resolved).map_err(read_error)?.is_file() {
        return Err(nonregular());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO swapped in after metadata must not block this open.
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options.open(&resolved).map_err(read_error)?;
    if !file.metadata().map_err(read_error)?.is_file() {
        return Err(nonregular());
    }
    Ok(file)
}
