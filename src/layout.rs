//! Screen layout: 100x30 at least, and the whole terminal when it is
//! bigger (as Armoured Commander I settled). The original's window is
//! 90x60 square cells; screens are laid out again for the terminal, so
//! the original's positions are scaled or moved, not kept.

/// The smallest screen the game lays out for.
pub const MIN_WIDTH: i32 = 100;
pub const MIN_HEIGHT: i32 = 30;

/// The original's window, in its square cells.
pub const WINDOW_WIDTH: i32 = 90;
pub const WINDOW_HEIGHT: i32 = 60;

static SCREEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(((MIN_WIDTH as u32) << 16) | MIN_HEIGHT as u32);

/// Set the screen size from the terminal's, never below 100x30.
pub fn set_screen_size(w: i32, h: i32) {
    let (w, h) = (w.max(MIN_WIDTH), h.max(MIN_HEIGHT));
    SCREEN.store(((w as u32) << 16) | h as u32, std::sync::atomic::Ordering::Relaxed);
}

/// The screen's width: the terminal's, at least 100.
pub fn sw() -> i32 {
    (SCREEN.load(std::sync::atomic::Ordering::Relaxed) >> 16) as i32
}

/// The screen's height: the terminal's, at least 30.
pub fn sh() -> i32 {
    (SCREEN.load(std::sync::atomic::Ordering::Relaxed) & 0xFFFF) as i32
}

pub fn sxm() -> i32 {
    sw() / 2
}
