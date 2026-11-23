//! Map a WebM timestamp to a cluster position using its bounded DASH index.

use std::time::Duration;

fn variable(data: &[u8], element_id: bool) -> Option<(u64, usize)> {
    let first = *data.first()?;
    let width = first.leading_zeros() + 1;
    let length = usize::try_from(width).ok()?;
    if length > if element_id { 4 } else { 8 } {
        return None;
    }
    let bytes = data.get(..length)?;
    let start = if element_id {
        first
    } else {
        first & u8::MAX.checked_shr(width).unwrap_or(0)
    };
    Some((
        bytes[1..].iter().fold(u64::from(start), |value, byte| {
            (value << 8) | u64::from(*byte)
        }),
        length,
    ))
}

fn header(data: &[u8]) -> Option<(u64, usize, usize)> {
    let (id, id_len) = variable(data, true)?;
    let (size, size_len) = variable(data.get(id_len..)?, false)?;
    Some((id, id_len + size_len, usize::try_from(size).ok()?))
}

fn elements(mut data: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    std::iter::from_fn(move || {
        let (id, prefix, size) = header(data)?;
        let end = prefix.checked_add(size)?;
        let body = data.get(prefix..end)?;
        data = data.get(end..)?;
        Some((id, body))
    })
}

fn unsigned(data: &[u8]) -> Option<u64> {
    (data.len() <= 8).then(|| {
        data.iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte))
    })
}

pub(super) fn cluster_at(initial: &[u8], index: &[u8], seconds: f64) -> Option<u64> {
    // The Segment payload can extend far past the initialization range.
    let mut data = initial;
    let (segment, base) = loop {
        let (id, prefix, size) = header(data)?;
        if id == 0x1853_8067 {
            break (
                data.get(prefix..)?,
                (initial.len() - data.len() + prefix) as u64,
            );
        }
        data = data.get(prefix.checked_add(size)?..)?;
    };
    let scale = elements(segment)
        .find(|(id, _)| *id == 0x1549_a966)
        .and_then(|(_, info)| elements(info).find(|(id, _)| *id == 0x2a_d7b1))
        .and_then(|(_, raw)| unsigned(raw))
        .and_then(|scale| u32::try_from(scale).ok())
        .filter(|scale| *scale != 0)
        .unwrap_or(1_000_000);
    let ticks = Duration::try_from_secs_f64(seconds).ok()?.as_nanos() / u128::from(scale);
    let (_, cues) = elements(index).find(|(id, _)| *id == 0x1c53_bb6b)?;
    let (_, position) = elements(cues)
        .filter(|(id, _)| *id == 0xbb)
        .filter_map(|(_, point)| {
            let time = unsigned(elements(point).find(|(id, _)| *id == 0xb3)?.1)?;
            let positions = elements(point).find(|(id, _)| *id == 0xb7)?.1;
            let offset = unsigned(elements(positions).find(|(id, _)| *id == 0xf1)?.1)?;
            (u128::from(time) <= ticks).then_some((time, offset))
        })
        .max_by_key(|(time, _)| *time)?;
    base.checked_add(position)
}

#[cfg(test)]
mod tests {
    use super::cluster_at;

    #[test]
    fn selects_the_cluster_at_or_before_the_saved_position() {
        let initialization = [0x18, 0x53, 0x80, 0x67, 0xff];
        let cues = [
            0x1c, 0x53, 0xbb, 0x6b, 0x97, 0xbb, 0x89, 0xb3, 0x81, 0, 0xb7, 0x84, 0xf1, 0x82, 0, 32,
            0xbb, 0x8a, 0xb3, 0x82, 3, 0xe8, 0xb7, 0x84, 0xf1, 0x82, 1, 0,
        ];
        assert_eq!(cluster_at(&initialization, &cues, 0.999), Some(37));
        assert_eq!(cluster_at(&initialization, &cues, 1.0), Some(261));
        assert_eq!(
            cluster_at(&initialization, &cues[..cues.len() - 1], 1.0),
            None
        );
    }
}
