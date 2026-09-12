use super::{Context, Result, check_cancel, err};
use crate::project::Package;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

pub(super) fn archive(
    cx: &Context<'_>,
    p: &Package,
    dir: &Path,
    executables: &[PathBuf],
) -> Result<PathBuf> {
    check_cancel(cx)?;
    let name = format!(
        "{}-{}-{}-{}.tar.gz",
        p.language.name(),
        cx.version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let path = dir.join(name);
    let mut entries = std::collections::BTreeMap::new();
    for file in executables {
        if !file.is_file() {
            return Err(format!(
                "Missing executable {}; run without --skip-build",
                file.display()
            ));
        }
        let name = file
            .file_name()
            .ok_or("Executable needs a filename")?
            .to_owned();
        if entries.insert(name, (file.clone(), 0o755)).is_some() {
            return Err("Binary names collide".into());
        }
    }
    for directory in [&p.directory, &cx.root, &cx.git_root] {
        for entry in fs::read_dir(directory).map_err(err)? {
            let entry = entry.map_err(err)?;
            let name = entry.file_name();
            let text = name.to_string_lossy().to_ascii_uppercase();
            if ["README", "LICENSE", "NOTICE"].iter().any(|prefix| {
                text == *prefix
                    || text.starts_with(&format!("{prefix}."))
                    || text.starts_with(&format!("{prefix}-"))
            }) && entry.path().is_file()
            {
                entries.entry(name).or_insert((entry.path(), 0o644));
            }
        }
    }
    // Fixed tar headers and gzip timestamp make repeated archives byte-identical.
    let temporary = tempfile::NamedTempFile::new_in(dir).map_err(err)?;
    let gzip = flate2::GzBuilder::new().mtime(0).write(
        temporary.reopen().map_err(err)?,
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(gzip);
    for (name, (file, mode)) in entries {
        check_cancel(cx)?;
        let mut input = File::open(file).map_err(err)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(input.metadata().map_err(err)?.len());
        header.set_mode(mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, Path::new(&name), &mut input)
            .map_err(err)?;
    }
    tar.into_inner().map_err(err)?.finish().map_err(err)?;
    temporary.persist(&path).map_err(err)?;
    Ok(path)
}
pub(super) fn checksum(path: &Path) -> Result<String> {
    hash_reader(File::open(path).map_err(err)?)
}
pub(super) fn hash_reader(mut input: impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = input.read(&mut buffer).map_err(err)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!(
        "sha256:{}",
        hash.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}
