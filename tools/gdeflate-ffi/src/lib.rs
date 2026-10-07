//! C ABI for decoding Rift Apart's GDeflate-compressed archive blocks (the DirectStorage tile
//! stream format), built from source via the `gdeflate` crate. `tools/extract_riftapart_ratchet.py`
//! loads this DLL instead of a prebuilt libdeflate.dll.
//!
//! Stream layout: `u8 id (4)`, `u8 id ^ 0xFF`, `u16 tile count`, `u32` (unused here), then one
//! `u32` per tile: entry 0 is the *last* tile's compressed size, entry i > 0 is tile i's offset.
//! Each tile decompresses to 64 KiB, except the last one.

use gdeflate::sys;

const GDEFLATE_ID: u8 = 4;
const TILE_SIZE: usize = 64 * 1024;

/// Decompresses one GDeflate stream from `src` into `dst` (`dst_len` = the expected size).
/// Returns the number of bytes written, or a negative error code.
///
/// # Safety
/// `src` must point to `src_len` readable bytes and `dst` to `dst_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rcra_gdeflate_decompress(src: *const u8, src_len: usize, dst: *mut u8, dst_len: usize) -> i64 {
    if src.is_null() || dst.is_null() || src_len < 8 {
        return -1;
    }
    let src = unsafe { std::slice::from_raw_parts(src, src_len) };
    let dst = unsafe { std::slice::from_raw_parts_mut(dst, dst_len) };
    match decompress(src, dst) {
        Ok(n) => n as i64,
        Err(code) => code,
    }
}

fn decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, i64> {
    let (id, magic) = (src[0], src[1]);
    if id != GDEFLATE_ID || id ^ magic != 0xFF {
        return Err(-2);
    }
    let tiles = u16::from_le_bytes([src[2], src[3]]) as usize;
    let table_end = 8 + 4 * tiles;
    if src.len() < table_end {
        return Err(-3);
    }
    let entry = |i: usize| u32::from_le_bytes(src[8 + 4 * i..12 + 4 * i].try_into().unwrap()) as usize;
    let data = &src[table_end..];

    let decompressor = unsafe { sys::libdeflate_alloc_gdeflate_decompressor() };
    if decompressor.is_null() {
        return Err(-4);
    }
    let mut written = 0;
    let mut result = Ok(());
    for i in 0..tiles {
        let start = if i == 0 { 0 } else { entry(i) };
        let size = if i + 1 < tiles { entry(i + 1).saturating_sub(start) } else { entry(0) };
        let out_start = i * TILE_SIZE;
        if start + size > data.len() || out_start >= dst.len() {
            result = Err(-5);
            break;
        }
        let out_len = TILE_SIZE.min(dst.len() - out_start);
        let mut page = sys::libdeflate_gdeflate_in_page { data: data[start..].as_ptr().cast(), nbytes: size };
        let mut actual = 0usize;
        let rc = unsafe {
            sys::libdeflate_gdeflate_decompress(
                decompressor,
                &mut page,
                1,
                dst[out_start..].as_mut_ptr().cast(),
                out_len,
                &mut actual,
            )
        };
        if rc != 0 {
            result = Err(-100 - rc as i64);
            break;
        }
        written = out_start + actual;
    }
    unsafe { sys::libdeflate_free_gdeflate_decompressor(decompressor) };
    result.map(|_| written)
}
