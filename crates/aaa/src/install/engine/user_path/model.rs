use super::*;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Value {
    pub kind: u32,
    pub bytes: Vec<u8>,
}
impl Value {
    fn units(&self) -> Result<Vec<u16>> {
        if !matches!(self.kind, 1 | 2) || self.bytes.len() > 65534 || !self.bytes.len().is_multiple_of(2) {
            return Err(Fault::conflict());
        }
        if self.bytes.is_empty() {
            return Ok(Vec::new());
        }
        let mut units: Vec<u16> =
            self.bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        if units.pop() != Some(0) || units.contains(&0) {
            return Err(Fault::conflict());
        }
        Ok(units)
    }
}
pub(super) fn desired(
    before: Option<&Value>,
    directory: &[u16],
    mut equal: impl FnMut(&[u16], &[u16], bool) -> Result<bool>,
) -> Result<Value> {
    if directory.is_empty() || directory.iter().any(|c| [0, b';' as u16, b'"' as u16].contains(c)) {
        return Err(Fault { code: "path_entry_unrepresentable", exit: 3 });
    }
    let mut units = before.map(Value::units).transpose()?.unwrap_or_default();
    if let Some(before) = before {
        for entry in units.split(|c| *c == b';' as u16) {
            if equal(entry, directory, before.kind == 2)? {
                return Ok(before.clone());
            }
        }
    }
    if !units.is_empty() {
        units.push(b';' as u16);
    }
    units.extend_from_slice(directory);
    units.push(0);
    if units.len() > 32767 {
        return Err(Fault { code: "path_registry_unavailable", exit: 3 });
    }
    Ok(Value { kind: before.map_or(2, |v| v.kind), bytes: units.into_iter().flat_map(u16::to_le_bytes).collect() })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn raw(kind: u32, text: &str) -> Value {
        Value { kind, bytes: text.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect() }
    }
    #[test]
    fn path_append_preserves_type_order_empty_entries_and_unpaired_units_and_never_repairs_bad_values() {
        let directory: Vec<u16> = r"C:\AAA\bin".encode_utf16().collect();
        for kind in [1, 2] {
            let before = raw(kind, r"C:\Other;;%APPDATA%\Tools;");
            let after = desired(Some(&before), &directory, |_, _, _| Ok(false)).unwrap();
            assert!(after.bytes.starts_with(&before.bytes[..before.bytes.len() - 2]));
            assert!(after == raw(kind, r"C:\Other;;%APPDATA%\Tools;;C:\AAA\bin"), "기존 항목·빈 항목·type를 유지한다");
            let mut unpaired = before.clone();
            unpaired.bytes.splice(0..0, 0xd800u16.to_le_bytes());
            let appended = desired(Some(&unpaired), &directory, |_, _, _| Ok(false)).unwrap();
            assert!(appended.bytes.starts_with(&unpaired.bytes[..unpaired.bytes.len() - 2]));
        }
        assert!(desired(None, &directory, |_, _, _| Ok(false)).unwrap() == raw(2, r"C:\AAA\bin"));
        for bad in [
            Value { kind: 4, bytes: vec![1, 0, 0, 0] },
            Value { kind: 1, bytes: vec![1] },
            Value { kind: 1, bytes: vec![65, 0] },
        ] {
            assert!(desired(Some(&bad), &directory, |_, _, _| Ok(false)).is_err());
        }
    }
}
