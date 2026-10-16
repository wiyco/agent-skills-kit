use super::*;
use std::io::{Read, Seek, SeekFrom, Write};

pub(super) fn payload(
    source: &ValidatedSkill,
    label: &Path,
    context: &Context,
    payload: &mut Payload,
    cleanup: &mut Vec<ApplyError>,
    hook: &mut impl FnMut(Phase, &Path) -> io::Result<()>,
) -> Result<(), ApplyError> {
    let root = source
        .inventory()
        .retained_root()
        .try_clone()
        .map_err(|observed| {
            io_error(
                source.inventory().requested_root(),
                "retain source traversal root",
                observed,
            )
        })?;
    let mut sources = BTreeMap::from([(PathBuf::new(), Arc::new(root))]);
    let mut modes = vec![(PathBuf::new(), source.inventory().root_stamp().mode & 0o755)];
    for entry in source.inventory().entries() {
        let relative = &entry.relative_path;
        let parent = relative.parent().expect("relative source parent");
        let name = relative.file_name().expect("relative source name");
        let input_parent = &sources[parent];
        let input_label = source.inventory().requested_root().join(relative);
        checkpoint(hook, Phase::BeforeCopyFile, &input_label)?;
        let input =
            native::child(input_parent, name, kind(entry.kind), false).map_err(|observed| {
                io_error(&input_label, "open approved source component", observed)
            })?;
        if metadata(&input, &input_label)? != entry.stamp
            || !bound(input_parent, name, &input, kind(entry.kind), &input_label)?
        {
            return Err(error(&input_label, ApplyErrorKind::SourceChanged));
        }
        let output_label = label.join(relative);
        let security = if entry.kind == SourceEntryKind::Directory {
            &context.directory
        } else {
            &context.file
        };
        let output = create(
            &payload.live[parent],
            name,
            kind(entry.kind),
            security,
            &output_label,
            cleanup,
        )?;
        payload.record(relative, entry.kind, output, &output_label)?;
        modes.push((relative.clone(), entry.output_mode));
        if entry.kind == SourceEntryKind::Directory {
            sources.insert(relative.clone(), Arc::new(input));
            continue;
        }
        let output = &payload.live[relative];
        let mut reader = &input;
        let mut writer = output.as_ref();
        let mut count = 0u64;
        let mut digest = blake3::Hasher::new();
        let mut buffer = [0u8; 8192];
        loop {
            let allowed = (entry.stamp.length - count)
                .saturating_add(1)
                .min(buffer.len() as u64) as usize;
            let read = match reader.read(&mut buffer[..allowed]) {
                Err(observed) if observed.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|observed| {
                    io_error(&input_label, "read approved source bytes", observed)
                })?,
            };
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > entry.stamp.length {
                return Err(error(&input_label, ApplyErrorKind::SourceChanged));
            }
            digest.update(&buffer[..read]);
            writer.write_all(&buffer[..read]).map_err(|observed| {
                io_error(&output_label, "write private payload bytes", observed)
            })?;
            checkpoint(hook, Phase::AfterWriteChunk, &output_label)?;
        }
        checkpoint(hook, Phase::AfterSourceRead, &input_label)?;
        if count != entry.stamp.length
            || Some(*digest.finalize().as_bytes()) != entry.digest
            || metadata(&input, &input_label)? != entry.stamp
            || !bound(input_parent, name, &input, Kind::File, &input_label)?
        {
            return Err(error(&input_label, ApplyErrorKind::SourceChanged));
        }
        let mut reader = output.as_ref();
        reader.seek(SeekFrom::Start(0)).map_err(|observed| {
            io_error(&output_label, "rewind staged byte verification", observed)
        })?;
        let mut count = 0u64;
        let mut digest = blake3::Hasher::new();
        loop {
            let allowed = (entry.stamp.length - count)
                .saturating_add(1)
                .min(buffer.len() as u64) as usize;
            let read = match reader.read(&mut buffer[..allowed]) {
                Err(observed) if observed.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|observed| {
                    io_error(&output_label, "read back staged bytes", observed)
                })?,
            };
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > entry.stamp.length {
                return Err(error(&output_label, ApplyErrorKind::StageChanged));
            }
            digest.update(&buffer[..read]);
        }
        if count != entry.stamp.length || Some(*digest.finalize().as_bytes()) != entry.digest {
            return Err(error(&output_label, ApplyErrorKind::StageChanged));
        }
    }
    for (relative, mode) in modes.into_iter().rev() {
        native::set_readonly(&payload.live[&relative], mode & 0o200 == 0).map_err(|observed| {
            io_error(
                &label.join(relative),
                "set owned ordinary read-only intent",
                observed,
            )
        })?;
    }
    Ok(())
}
