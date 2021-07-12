use core::fmt;
use core::fmt::Write;

use font8x8::UnicodeFonts;

use spin::mutex::SpinMutex;
use spin::Once;

/// Describes the layout and pixel format of a framebuffer.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FrameBufferInfo {
    /// The width in pixels.
    pub horizontal_resolution: usize,
    /// The height in pixels.
    pub vertical_resolution: usize,
    /// The color format of each pixel.
    pub pixel_format: PixelFormat,
    /// The number of bits per pixel.
    pub bits_per_pixel: usize,
    /// Number of pixels between the start of a line and the start of the next.
    ///
    /// Some framebuffers use additional padding at the end of a line, so this
    /// value might be larger than `horizontal_resolution`. It is
    /// therefore recommended to use this field for calculating the start address of a line.
    pub stride: usize,
}

/// Color format of pixels in the framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[repr(C)]
pub enum PixelFormat {
    /// One byte red, then one byte green, then one byte blue.
    ///
    /// Length might be larger than 3, check [`bytes_per_pixel`][FrameBufferInfo::bytes_per_pixel]
    /// for this.
    RGB,
    /// One byte blue, then one byte green, then one byte red.
    ///
    /// Length might be larger than 3, check [`bytes_per_pixel`][FrameBufferInfo::bytes_per_pixel]
    /// for this.
    BGR,
}

/// The global logger instance used for the `log` crate.
pub static LOGGER: Once<LockedLogger> = Once::new();

/// A [`Logger`] instance protected by a spinlock.
pub struct LockedLogger(SpinMutex<Logger>);

impl LockedLogger {
    /// Create a new instance that logs to the given framebuffer.
    #[inline]
    pub fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        Self(SpinMutex::new(Logger::new(framebuffer, info)))
    }

    /// Force-unlocks the logger to prevent a deadlock.
    ///
    /// ## Saftey
    /// This method is not memory safe and should be only used when absolutely necessary.
    pub unsafe fn force_unlock(&self) {
        self.0.force_unlock()
    }
}

impl log::Log for LockedLogger {
    #[inline]
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    #[inline]
    fn log(&self, record: &log::Record) {
        let mut logger = self.0.lock();
        writeln!(logger, "{}:    {}", record.level(), record.args()).unwrap();
    }

    #[inline]
    fn flush(&self) {}
}

struct Logger {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,

    x_pos: usize,
    y_pos: usize,

    scroll_lock: bool,
}

impl Logger {
    #[inline]
    fn new(framebuffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        Self {
            framebuffer,
            info,

            x_pos: 0x00,
            y_pos: 0x00,

            scroll_lock: false,
        }
    }

    fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.new_line(),
            '\r' => self.carriage_return(),
            _ => {
                if self.x_pos >= self.width() {
                    self.new_line();
                }

                if self.y_pos >= (self.height() - 16) {
                    self.clear();
                }

                let rendered = font8x8::BASIC_FONTS
                    .get(c)
                    .expect("Character not found in basic font");

                self.write_rendered_char(rendered);
            }
        }
    }

    fn write_rendered_char(&mut self, rendered: [u8; 8]) {
        for (y, byte) in rendered.iter().enumerate() {
            for (x, bit) in (0..8).enumerate() {
                let alpha = if *byte & (1 << bit) == 0 { 0 } else { 255 };
                self.write_pixel(self.x_pos + x, self.y_pos + y, alpha);
            }
        }

        self.x_pos += 8;
    }

    fn write_pixel(&mut self, x: usize, y: usize, intensity: u8) {
        let pixel_offset = y * self.info.stride + x;
        let color = match self.info.pixel_format {
            PixelFormat::RGB => [intensity, intensity, intensity / 2, 0],
            PixelFormat::BGR => [intensity / 2, intensity, intensity, 0],
        };

        let bits_per_pixel = self.info.bits_per_pixel;
        let byte_offset = pixel_offset * bits_per_pixel;

        self.framebuffer[byte_offset..(byte_offset + bits_per_pixel)]
            .copy_from_slice(&color[..bits_per_pixel]);

        let _ = unsafe { core::ptr::read_volatile(&self.framebuffer[byte_offset]) };
    }

    #[inline]
    fn clear(&mut self) {
        self.x_pos = 0;
        self.y_pos = 0;

        self.framebuffer.fill(0x00)
    }

    #[inline]
    fn width(&self) -> usize {
        self.info.horizontal_resolution
    }

    #[inline]
    fn height(&self) -> usize {
        self.info.vertical_resolution
    }

    #[inline]
    fn carriage_return(&mut self) {
        self.x_pos = 0;
    }

    #[inline]
    fn new_line(&mut self) {
        if !self.scroll_lock {
            self.y_pos += 16;
        }

        self.carriage_return();
    }
}

impl fmt::Write for Logger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c)
        }

        Ok(())
    }
}

/// This function is responsible for initializing the global logger
/// instance.
pub fn init(framebuffer: &'static mut [u8], info: FrameBufferInfo) {
    let logger = LOGGER.call_once(move || LockedLogger::new(framebuffer, info));

    log::set_logger(logger).expect("Logger already set");
    log::set_max_level(log::LevelFilter::Trace);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::logger::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::rendy::print!("\n"));
    ($($arg:tt)*) => ($crate::prelude::print!("{}\n", format_args!($($arg)*)));
}

/// This function is responsible for clearing the screen.
pub fn clear() {
    LOGGER.get().map(|l| l.0.lock().clear());
}

pub fn set_scroll_lock(lock: bool) {
    LOGGER.get().map(|l| l.0.lock().scroll_lock = lock);
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    LOGGER.get().map(|l| l.0.lock().write_fmt(args));
}
