/// Parse an IDA-style signature string into a (pattern, mask) pair.
/// Wildcards are `?` or `??`. Example: "48 8B 05 ?? ?? ?? ?? 48 85 C0"
fn parse_sig(sig: &str) -> (Vec<u8>, Vec<bool>) {
    let mut pattern = Vec::new();
    let mut mask = Vec::new();

    for token in sig.split_whitespace() {
        if token.contains('?') {
            pattern.push(0u8);
            mask.push(false); // wildcard — skip comparison
        } else {
            pattern.push(u8::from_str_radix(token, 16).unwrap_or(0));
            mask.push(true); // must match
        }
    }

    (pattern, mask)
}

/// Scan `bytes` (read from base_addr in the target process) for the given
/// IDA-style signature. Returns the virtual address of the match.
pub fn scan(bytes: &[u8], base_addr: usize, sig: &str) -> Option<usize> {
    let (pattern, mask) = parse_sig(sig);
    let pat_len = pattern.len();

    'outer: for i in 0..bytes.len().saturating_sub(pat_len) {
        for j in 0..pat_len {
            if mask[j] && bytes[i + j] != pattern[j] {
                continue 'outer;
            }
        }
        return Some(base_addr + i);
    }

    None
}

/// Resolve a RIP-relative address found at `match_offset` within `bytes`.
///
/// Mirrors C++ `ResolveRelativeAddress`:
///   RVA  = i32 at bytes[match_offset + rva_offset]
///   RIP  = base_addr + match_offset + rip_offset   (address after instruction)
///   result = RIP + RVA
pub fn resolve_rip(
    bytes: &[u8],
    base_addr: usize,
    match_offset: usize,
    rva_offset: usize,
    rip_offset: usize,
) -> Option<usize> {
    let rva_pos = match_offset + rva_offset;
    let rva = i32::from_le_bytes(bytes.get(rva_pos..rva_pos + 4)?.try_into().ok()?);
    let rip = base_addr + match_offset + rip_offset;
    Some((rip as isize + rva as isize) as usize)
}

/// Resolve an absolute address found at `match_offset` within `bytes`.
///
/// Mirrors C++ `GetAbsoluteAddress`:
///   addr   = match_offset + pre_offset
///   result = base_addr + addr + 4 + i32_at(bytes, addr) + post_offset
pub fn get_absolute(
    bytes: &[u8],
    base_addr: usize,
    match_offset: usize,
    pre_offset: usize,
    post_offset: usize,
) -> Option<usize> {
    let addr = match_offset + pre_offset;
    let relative = i32::from_le_bytes(bytes.get(addr..addr + 4)?.try_into().ok()?);
    Some(base_addr + addr + 4 + relative as usize + post_offset)
}
