use crate::pmem;
use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::slice;
use alloc::string::String;
use core::ffi::{c_char, c_int, c_ulonglong, c_void, CStr};
use core::ptr;
use corundum::ll;

struct File {
    filename: String,
    mode: String,
    pos: u64,
}

#[no_mangle]
extern "C" fn fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void {
    let filename = match unsafe { CStr::from_ptr(filename) }.to_str() {
        Ok(filename) => filename,
        Err(_) => return ptr::null_mut(),
    };
    let mode = match unsafe { CStr::from_ptr(mode) }.to_str() {
        Ok(filename) => filename,
        Err(_) => return ptr::null_mut(),
    };
    let mut mgr = pmem::MANAGER.lock();

    if mgr
        .get_pool(filename)
        .or_else(|| {
            if mode.contains(['w', 'a']) {
                mgr.create_pool(filename, 0)
            } else {
                None
            }
        })
        .is_some()
    {
        Box::into_raw(Box::new(File {
            filename: filename.to_owned(),
            mode: mode.to_owned(),
            pos: 0,
        })) as *mut c_void
    } else {
        ptr::null_mut()
    }
}

#[no_mangle]
extern "C" fn fwrite(buf: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize {
    let file = unsafe { Box::<File>::from_raw(file as *mut File) };
    let buf_size = size * count;
    let written = if file.mode.contains(['w', 'a', '+']) {
        pmem::MANAGER
            .lock()
            .get_pool(&file.filename)
            .and_then(|(addr, size)| {
                unsafe { slice::from_raw_parts_mut(addr as *mut u8, size as usize) }
                    .get_mut(file.pos as usize..)
                    .map(|s| {
                        let amt = buf_size.min(s.len());
                        let buf = unsafe { slice::from_raw_parts_mut(buf as *mut u8, buf_size) };
                        s[..amt].copy_from_slice(&buf[..amt]);
                        amt
                    })
            })
            .unwrap_or(0)
    } else {
        0
    };

    Box::leak(file);
    written
}

#[no_mangle]
extern "C" fn fclose(file: *mut c_void) -> c_int {
    let _ = unsafe { Box::<File>::from_raw(file as *mut File) };
    0
}

#[no_mangle]
extern "C" fn remove(filename: *const c_char) -> c_int {
    let filename = match unsafe { CStr::from_ptr(filename) }.to_str() {
        Ok(filename) => filename,
        Err(_) => return -1,
    };

    if pmem::MANAGER.lock().destroy_pool(filename) {
        0
    } else {
        -1
    }
}

#[no_mangle]
extern "C" fn truncate(filename: *const c_char, length: c_ulonglong) -> c_ulonglong {
    let filename = match unsafe { CStr::from_ptr(filename) }.to_str() {
        Ok(filename) => filename,
        Err(_) => return 0,
    };

    if let Some((addr, new_length)) = pmem::MANAGER.lock().resize_pool(filename, length) {
        let buf = unsafe { slice::from_raw_parts_mut(addr as *mut u8, new_length as usize) };
        let extended = &mut buf[length as usize..];

        if !extended.is_empty() {
            extended.fill(0);
            ll::persist_obj(extended, true);
        }

        new_length
    } else {
        length
    }
}

#[no_mangle]
extern "C" fn size(filename: *const c_char) -> c_ulonglong {
    let filename = match unsafe { CStr::from_ptr(filename) }.to_str() {
        Ok(filename) => filename,
        Err(_) => return 0,
    };

    let size = pmem::MANAGER
        .lock()
        .get_pool(filename)
        .map(|(_, size)| size as c_ulonglong)
        .unwrap_or(0);
    size
}

#[no_mangle]
extern "C" fn map(filename: *const c_char) -> *mut c_void {
    let filename = match unsafe { CStr::from_ptr(filename) }.to_str() {
        Ok(filename) => filename,
        Err(_) => return ptr::null_mut(),
    };

    let r = pmem::MANAGER
        .lock()
        .get_pool(filename)
        .map(|(addr, _)| addr as *mut c_void)
        .unwrap_or(ptr::null_mut());

    r
}

#[no_mangle]
extern "C" fn unmap(_addr: *mut c_void) -> c_int {
    0
}
