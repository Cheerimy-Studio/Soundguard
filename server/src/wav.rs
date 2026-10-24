/// Validate the PCM WAV format emitted by the collectors and return its duration.
pub fn duration(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u32_at = |offset| -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    let u16_at = |offset| -> Option<u16> {
        Some(u16::from_le_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ))
    };
    if u32_at(4)? as usize + 8 != bytes.len() {
        return None;
    }
    let mut offset = 12usize;
    let mut byte_rate = None;
    let mut alignment = None;
    let mut data_len = None;
    while offset.checked_add(8)? <= bytes.len() {
        let length = u32_at(offset + 4)? as usize;
        let start = offset.checked_add(8)?;
        let end = start.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        match &bytes[offset..offset + 4] {
            b"fmt " => {
                if length < 16 || u16_at(start)? != 1 {
                    return None;
                }
                let channels = u16_at(start + 2)?;
                let sample_rate = u32_at(start + 4)?;
                let rate = u32_at(start + 8)?;
                let align = u16_at(start + 12)?;
                if !(1..=2).contains(&channels)
                    || !(8000..=192000).contains(&sample_rate)
                    || u16_at(start + 14)? != 16
                    || align != channels * 2
                    || rate != sample_rate * u32::from(align)
                {
                    return None;
                }
                byte_rate = Some(rate);
                alignment = Some(align);
            }
            b"data" if data_len.replace(length).is_some() => {
                return None;
            }
            _ => {}
        }
        offset = end.checked_add(length % 2)?;
    }
    let length = data_len?;
    if length == 0 || length % usize::from(alignment?) != 0 {
        return None;
    }
    Some(length as f64 / f64::from(byte_rate?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_headers_and_riff_wrappers() {
        for len in 0..64 {
            assert!(duration(&vec![0; len]).is_none());
        }
        let mut bytes = vec![0; 48];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[8..12].copy_from_slice(b"WAVE");
        assert!(duration(&bytes).is_none());
    }
}
