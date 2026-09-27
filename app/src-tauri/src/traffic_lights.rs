//! macOS: centers the traffic lights vertically in the UI's title bar.
//!
//! `trafficLightPosition` in the window config is applied by tao only while its
//! own view draws, which the webview covers, so the buttons stayed at the top.
//! AppKit also lays the title bar out again on resize, focus and theme changes,
//! so this runs after each of those.

use objc2_app_kit::{NSWindow, NSWindowButton};
use tauri::{Runtime, Window};

/// Height of the title bar in the UI (`h-11` in `TitleBar.tsx`).
const TITLE_BAR_HEIGHT: f64 = 44.0;
/// Close button's distance from the window's left edge: the same as from the top.
const LEFT: f64 = 14.0;

pub fn center<R: Runtime>(window: &Window<R>) {
    let Ok(ptr) = window.ns_window() else { return };
    // A raw pointer is not `Send`; the window outlives this queued call.
    let ptr = ptr as usize;
    let _ = window.run_on_main_thread(move || {
        // SAFETY: tauri's pointer to the live NSWindow, used on the main thread.
        let ns_window = unsafe { &*(ptr as *const NSWindow) };
        center_buttons(ns_window);
    });
}

fn center_buttons(window: &NSWindow) {
    let buttons = [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton]
        .map(|kind| window.standardWindowButton(kind));
    let Some(close) = &buttons[0] else { return };
    // The buttons sit in a title bar view inside a container along the top of the window:
    // make the container as tall as the UI's bar, then center each button in it.
    // SAFETY: plain view hierarchy reads on the main thread.
    let Some(container) = (unsafe { close.superview().and_then(|view| view.superview()) }) else { return };
    let mut frame = container.frame();
    frame.size.height = TITLE_BAR_HEIGHT;
    frame.origin.y = window.frame().size.height - TITLE_BAR_HEIGHT;
    container.setFrame(frame);
    // A taller container moves the buttons left; shift all three by the same amount so
    // the close button is `LEFT` from the window edge (measured in window coordinates,
    // so running this again changes nothing).
    let dx = LEFT - close.convertRect_toView(close.bounds(), None).origin.x;
    for button in buttons.iter().flatten() {
        let rect = button.frame();
        let mut origin = rect.origin;
        origin.x += dx;
        origin.y = ((TITLE_BAR_HEIGHT - rect.size.height) / 2.0).round();
        button.setFrameOrigin(origin);
    }
}
