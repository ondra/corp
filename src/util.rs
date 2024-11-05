pub fn as_slice_ref<'a, T>(mmap: &'a memmap::Mmap) -> &'a [T] {
    unsafe{ std::slice::from_raw_parts(
        mmap.as_ptr() as *const T,
        (mmap.len() + (std::mem::size_of::<T>()-1)) / std::mem::size_of::<T>())
    }
}

