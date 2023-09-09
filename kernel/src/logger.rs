use crate::framebuffer::FrameBufferWriter;
use bootloader_api::info::{FrameBuffer, FrameBufferInfo};
use conquer_once::spin::OnceCell;
use core::fmt::{self, Write};
use spinning_top::Spinlock;

/// The global logger instance used for the `log` crate.
pub static LOGGER: OnceCell<LockedLogger> = OnceCell::uninit();

/// A Logger instance protected by a spinlock.
pub struct LockedLogger(Spinlock<FrameBufferWriter>);

impl LockedLogger {
    /// Create a new instance that logs to the given framebuffer.
    pub fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        LockedLogger(Spinlock::new(FrameBufferWriter::new(framebuffer, info)))
    }

    /// Force-unlocks the logger to prevent a deadlock.
    ///
    /// This method is not memory safe and should be only used when absolutely
    /// necessary.
    pub unsafe fn force_unlock(&self) {
        self.0.force_unlock();
    }
}

impl log::Log for LockedLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        use x86_64::instructions::interrupts;

        interrupts::without_interrupts(|| {
            let mut logger = self.0.lock();
            writeln!(logger, "{:5}: {}", record.level(), record.args()).unwrap();
        });
    }

    fn flush(&self) {}
}

pub fn init(framebuffer: &'static mut FrameBuffer) {
    let info = framebuffer.info();
    let logger = LOGGER.get_or_init(move || LockedLogger::new(framebuffer.buffer_mut(), info));
    log::set_logger(logger).expect("logger already set");
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::logger::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::println!(""));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => ($crate::println!("ERROR: {}", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        LOGGER.get().unwrap().0.lock().write_fmt(args).unwrap()
    });
}

mod ffi {
    use core::ffi::{c_char, c_int, CStr};

    #[no_mangle]
    fn putchar(ch: c_int) -> c_int {
        print!(
            "{}",
            ch.try_into()
                .ok()
                .and_then(char::from_u32)
                .unwrap_or(char::REPLACEMENT_CHARACTER)
        );
        0
    }

    #[no_mangle]
    fn puts(s: *const c_char) -> c_int {
        match unsafe { CStr::from_ptr(s) }.to_str() {
            Ok(s) => {
                println!("{}", s);
                0
            }
            Err(_) => -1,
        }
    }

    #[no_mangle]
    fn perror(s: *const c_char) {
        if let Ok(s) = unsafe { CStr::from_ptr(s) }.to_str() {
            eprintln!("{}", s);
        }
    }
}
