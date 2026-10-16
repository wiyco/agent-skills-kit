use super::*;
use std::io::{Read, Write};

pub(super) fn payload(
    source: &ValidatedSkill,
    root: &Arc<File>,
    label: &Path,
    objects: &mut Vec<Owned>,
    cleanup: &mut Vec<ApplyError>,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<(), ApplyError> {
    let mut sources = BTreeMap::from([(
        PathBuf::new(),
        Arc::clone(source.inventory().retained_root()),
    )]);
    let mut outputs = BTreeMap::from([(PathBuf::new(), Arc::clone(root))]);
    let mut modes = vec![(
        Arc::clone(root),
        source.inventory().root_stamp().mode & 0o755,
        label.to_owned(),
    )];
    for entry in source.inventory().entries() {
        let relative = &entry.relative_path;
        let parent = relative.parent().expect("relative resource parent");
        let name = relative.file_name().expect("relative resource name");
        let input_parent = sources
            .get(parent)
            .expect("inventory parent precedes child");
        let output_parent = outputs.get(parent).expect("created parent precedes child");
        let input_label = source.inventory().requested_root().join(relative);
        checkpoint(hook, Phase::BeforeCopyFile, &input_label)?;
        let input_flags = flags()
            | if entry.kind == SourceEntryKind::Directory {
                OFlags::DIRECTORY
            } else {
                OFlags::empty()
            };
        let descriptor = rustix::fs::openat(input_parent, name, input_flags, Mode::empty())
            .map_err(|observed| {
                io_error(&input_label, "open approved source component", observed)
            })?;
        let input = Arc::new(File::from(descriptor));
        if metadata(&input, &input_label)? != entry.stamp
            || !bound(input_parent, name, &input, &input_label)?
        {
            return Err(error(&input_label, ApplyErrorKind::SourceChanged));
        }
        let output_label = label.join(relative);
        if entry.kind == SourceEntryKind::Directory {
            let owned = create_directory(output_parent, name, &output_label, cleanup)?;
            outputs.insert(relative.clone(), Arc::clone(&owned.file));
            modes.push((Arc::clone(&owned.file), entry.output_mode, output_label));
            objects.push(owned);
            sources.insert(relative.clone(), input);
        } else {
            ensure_no_acl(output_parent, &output_label)?;
            let descriptor = rustix::fs::openat(
                output_parent,
                name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|observed| {
                io_error(&output_label, "create exclusive payload file", observed)
            })?;
            let output = Arc::new(File::from(descriptor));
            let output_stamp = metadata(&output, &output_label)?;
            objects.push(Owned {
                parent: Arc::clone(output_parent),
                name: name.to_owned(),
                file: Arc::clone(&output),
                path: output_label.clone(),
                kind: SourceEntryKind::File,
                identity: id(&output_stamp),
            });
            ensure_no_acl(&output, &output_label)?;
            let mut reader = input.as_ref();
            let mut writer = output.as_ref();
            let mut total = 0u64;
            let mut hash = blake3::Hasher::new();
            let mut buffer = [0u8; 8192];
            loop {
                let allowed = (entry.stamp.length - total)
                    .saturating_add(1)
                    .min(buffer.len() as u64) as usize;
                let count = match reader.read(&mut buffer[..allowed]) {
                    Err(observed) if observed.kind() == io::ErrorKind::Interrupted => continue,
                    result => result.map_err(|observed| {
                        io_error(&input_label, "read approved source bytes", observed)
                    })?,
                };
                if count == 0 {
                    break;
                }
                total += count as u64;
                if total > entry.stamp.length {
                    return Err(error(&input_label, ApplyErrorKind::SourceChanged));
                }
                hash.update(&buffer[..count]);
                writer.write_all(&buffer[..count]).map_err(|observed| {
                    io_error(&output_label, "write staged payload", observed)
                })?;
                checkpoint(hook, Phase::AfterWriteChunk, &output_label)?;
            }
            checkpoint(hook, Phase::AfterSourceRead, &input_label)?;
            if total != entry.stamp.length
                || Some(*hash.finalize().as_bytes()) != entry.digest
                || metadata(&input, &input_label)? != entry.stamp
                || !bound(input_parent, name, &input, &input_label)?
            {
                return Err(error(&input_label, ApplyErrorKind::SourceChanged));
            }
            // Read the actual staged file back before any restrictive final
            // modes. Retained file offsets are explicitly reset, not shared
            // with source descriptors or inferred from written byte counts.
            rustix::fs::seek(&output, rustix::fs::SeekFrom::Start(0)).map_err(|observed| {
                io_error(&output_label, "rewind staged verification", observed)
            })?;
            let mut reader = output.as_ref();
            let mut hash = blake3::Hasher::new();
            let mut verified = 0u64;
            loop {
                let allowed = (entry.stamp.length - verified)
                    .saturating_add(1)
                    .min(buffer.len() as u64) as usize;
                let count = match reader.read(&mut buffer[..allowed]) {
                    Err(observed) if observed.kind() == io::ErrorKind::Interrupted => continue,
                    result => result.map_err(|observed| {
                        io_error(&output_label, "verify staged bytes", observed)
                    })?,
                };
                if count == 0 {
                    break;
                }
                verified += count as u64;
                if verified > entry.stamp.length {
                    return Err(error(&output_label, ApplyErrorKind::StageChanged));
                }
                hash.update(&buffer[..count]);
            }
            if verified != entry.stamp.length || Some(*hash.finalize().as_bytes()) != entry.digest {
                return Err(error(&output_label, ApplyErrorKind::StageChanged));
            }
            modes.push((output, entry.output_mode, output_label));
        }
    }
    // Children are complete before directory permissions become restrictive.
    for (file, mode, path) in modes.into_iter().rev() {
        // RawMode is u16 on Apple and u32 on Linux; every value is capped 0755.
        #[allow(clippy::unnecessary_cast)]
        let native_mode = Mode::from_raw_mode(mode as _);
        rustix::fs::fchmod(&file, native_mode)
            .map_err(|observed| io_error(&path, "apply capped ordinary payload mode", observed))?;
        if metadata(&file, &path)?.mode & 0o7777 != mode {
            return Err(error(&path, ApplyErrorKind::StageChanged));
        }
    }
    Ok(())
}
