//! In-place cmap rewrite that drops Private Use Area mappings from a TrueType face.

const PUA_FIRST: u32 = 0xE000;
const PUA_LAST: u32 = 0xF8FF;

/// `(first codepoint, last codepoint, first glyph id)` of a format 12 sequential map group.
type Group = (u32, u32, u32);

/// `font` without BMP Private Use Area cmap entries, or `None` if the cmap cannot be rewritten in place.
pub(super) fn without_private_use(font: &[u8]) -> Option<Vec<u8>> {
    let (record, offset, length) = table_record(font, b"cmap")?;
    let cmap = font.get(offset..offset.checked_add(length)?)?;
    let groups = format12_groups(cmap)?;
    let kept: Vec<Group> = groups.into_iter().flat_map(outside_pua).collect();
    let table = encode_cmap(&kept);
    if table.len() > length {
        return None;
    }

    let mut out = font.to_vec();
    let slot = out.get_mut(offset..offset + length)?;
    slot.fill(0);
    slot[..table.len()].copy_from_slice(&table);
    write_u32(&mut out, record + 4, checksum(&table))?;
    write_u32(&mut out, record + 12, u32::try_from(table.len()).ok()?)?;
    Some(out)
}

/// Every codepoint the face's format 12 subtable maps to a glyph.
#[cfg(test)]
pub(super) fn mapped_codepoints(font: &[u8]) -> Option<Vec<u32>> {
    let (_, offset, length) = table_record(font, b"cmap")?;
    let cmap = font.get(offset..offset.checked_add(length)?)?;
    let groups = format12_groups(cmap)?;
    Some(
        groups
            .into_iter()
            .flat_map(|(first, last, _)| first..=last)
            .collect(),
    )
}

/// Index of the table's directory record, plus the table's offset and length.
fn table_record(font: &[u8], tag: &[u8; 4]) -> Option<(usize, usize, usize)> {
    let count = usize::from(read_u16(font, 4)?);
    (0..count).find_map(|i| {
        let record = 12 + 16 * i;
        if font.get(record..record + 4)? != tag {
            return None;
        }
        let offset = usize::try_from(read_u32(font, record + 8)?).ok()?;
        let length = usize::try_from(read_u32(font, record + 12)?).ok()?;
        Some((record, offset, length))
    })
}

fn format12_groups(cmap: &[u8]) -> Option<Vec<Group>> {
    let encodings = usize::from(read_u16(cmap, 2)?);
    let subtable = (0..encodings).find_map(|i| {
        let record = 4 + 8 * i;
        let platform = read_u16(cmap, record)?;
        let encoding = read_u16(cmap, record + 2)?;
        let offset = usize::try_from(read_u32(cmap, record + 4)?).ok()?;
        let unicode_full = matches!((platform, encoding), (3, 10) | (0, 4) | (0, 6));
        (unicode_full && read_u16(cmap, offset)? == 12).then_some(offset)
    })?;
    let count = usize::try_from(read_u32(cmap, subtable + 12)?).ok()?;
    (0..count)
        .map(|i| {
            let at = subtable + 16 + 12 * i;
            Some((
                read_u32(cmap, at)?,
                read_u32(cmap, at + 4)?,
                read_u32(cmap, at + 8)?,
            ))
        })
        .collect()
}

/// The parts of `group` below and above the BMP Private Use Area.
fn outside_pua((first, last, glyph): Group) -> impl Iterator<Item = Group> {
    let below = (first < PUA_FIRST).then(|| (first, last.min(PUA_FIRST - 1), glyph));
    let above = (last > PUA_LAST).then(|| {
        let start = first.max(PUA_LAST + 1);
        (start, last, glyph + (start - first))
    });
    below.into_iter().chain(above)
}

/// A cmap holding one Windows Unicode full-repertoire format 12 subtable.
fn encode_cmap(groups: &[Group]) -> Vec<u8> {
    let subtable_len = 16 + 12 * groups.len();
    let mut out = Vec::with_capacity(12 + subtable_len);
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&10u16.to_be_bytes());
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(subtable_len as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(groups.len() as u32).to_be_bytes());
    for (first, last, glyph) in groups {
        out.extend_from_slice(&first.to_be_bytes());
        out.extend_from_slice(&last.to_be_bytes());
        out.extend_from_slice(&glyph.to_be_bytes());
    }
    out
}

/// OpenType table checksum: wrapping sum of big-endian words, zero-padded to a word boundary.
fn checksum(table: &[u8]) -> u32 {
    table.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn write_u32(bytes: &mut [u8], at: usize, value: u32) -> Option<()> {
    bytes
        .get_mut(at..at + 4)?
        .copy_from_slice(&value.to_be_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_straddling_the_pua_keeps_both_ends_and_their_glyphs() {
        let parts: Vec<Group> = outside_pua((0xDFFE, 0xF901, 100)).collect();
        let pua_span = PUA_LAST + 1 - 0xDFFE;
        assert_eq!(
            parts,
            vec![(0xDFFE, 0xDFFF, 100), (0xF900, 0xF901, 100 + pua_span)]
        );
    }

    #[test]
    fn a_group_inside_the_pua_disappears() {
        assert_eq!(outside_pua((0xE010, 0xE020, 5)).count(), 0);
    }

    #[test]
    fn a_group_clear_of_the_pua_is_untouched() {
        let parts: Vec<Group> = outside_pua((0x41, 0x5A, 36)).collect();
        assert_eq!(parts, vec![(0x41, 0x5A, 36)]);
    }

    #[test]
    fn a_font_without_a_cmap_is_left_alone() {
        assert!(without_private_use(&[0u8; 12]).is_none());
    }
}
