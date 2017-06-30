use std::ptr;

#[inline]
pub unsafe fn ptr_copy(src: *const u8, dst: *mut u8, count: usize, is_overlapping: bool) {
    if !is_overlapping {
        ptr::copy_nonoverlapping(src, dst, count);
    } else {
        for i in 0..count {
            ptr::copy_nonoverlapping(src.offset(i as isize), dst.offset(i as isize), 1);
        }
    }
}
