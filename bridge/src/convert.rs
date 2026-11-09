pub(crate) fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).expect("usize is at least 32 bits on every supported target")
}
