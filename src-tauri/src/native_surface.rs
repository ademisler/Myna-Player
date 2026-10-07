use myna_player_core::VideoSurfaceRect;
use tauri::WebviewWindow;

#[derive(Debug, Clone, Copy)]
pub struct NativeVideoSurface {
    handle: usize,
}

impl NativeVideoSurface {
    pub fn create(window: &WebviewWindow) -> Result<Self, String> {
        create_surface(window).map(|handle| Self { handle })
    }

    pub fn handle(&self) -> usize {
        self.handle
    }

    pub fn from_handle(handle: usize) -> Self {
        Self { handle }
    }

    pub fn set_rect(&self, window: &WebviewWindow, rect: VideoSurfaceRect) -> Result<(), String> {
        set_surface_rect(window, self.handle, rect)
    }
}

#[cfg(target_os = "macos")]
fn create_surface(window: &WebviewWindow) -> Result<usize, String> {
    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{NSColor, NSView, NSWindow, NSWindowOrderingMode};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let webview_pointer = window.ns_view().map_err(|error| error.to_string())?;
    if webview_pointer.is_null() {
        return Err("Tauri returned a null macOS webview".into());
    }
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "native video surface must be created on the main thread".to_string())?;
    let window_pointer = window.ns_window().map_err(|error| error.to_string())?;
    if !window_pointer.is_null() {
        // SAFETY: Tauri owns the NSWindow for the lifetime of this WebviewWindow.
        let native_window = unsafe { &*(window_pointer.cast::<NSWindow>()) };
        native_window.setBackgroundColor(Some(&NSColor::blackColor()));
    }

    // SAFETY: Tauri owns this NSView for the window lifetime and setup runs on the main thread.
    let webview = unsafe { &*(webview_pointer.cast::<NSView>()) };
    // SAFETY: Tauri's webview is attached to its window content view during setup.
    let parent = unsafe { webview.superview() }
        .ok_or_else(|| "Tauri webview has no parent NSView".to_string())?;
    let video = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)),
    );
    video.setHidden(true);
    parent.addSubview_positioned_relativeTo(&video, NSWindowOrderingMode::Below, Some(webview));

    Ok(Retained::into_raw(video) as usize)
}

#[cfg(target_os = "macos")]
fn set_surface_rect(
    window: &WebviewWindow,
    handle: usize,
    rect: VideoSurfaceRect,
) -> Result<(), String> {
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    if handle == 0 || rect.width < 0.0 || rect.height < 0.0 {
        return Err("invalid native video surface rectangle".into());
    }
    let surface_handle = handle;
    window
        .run_on_main_thread(move || {
            // SAFETY: The handle is the leaked application-lifetime NSView created above.
            let surface = unsafe { &*(surface_handle as *const NSView) };
            if rect.width <= 1.0 || rect.height <= 1.0 {
                surface.setHidden(true);
                surface.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0)));
                return;
            }
            // Web coordinates start at the top-left; AppKit coordinates start at the bottom-left.
            // SAFETY: The application-lifetime surface remains attached to its superview.
            let parent_height = unsafe { surface.superview() }
                .map(|parent| parent.frame().size.height)
                .unwrap_or(rect.y + rect.height);
            let appkit_y = (parent_height - rect.y - rect.height).max(0.0);
            surface.setFrame(NSRect::new(
                NSPoint::new(rect.x, appkit_y),
                NSSize::new(rect.width, rect.height),
            ));
            surface.setHidden(false);
        })
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn create_surface(window: &WebviewWindow) -> Result<usize, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, HWND_BOTTOM, SWP_NOACTIVATE, SetWindowPos, WS_CHILD, WS_CLIPCHILDREN,
            WS_CLIPSIBLINGS,
        },
    };

    let parent = window.hwnd().map_err(|error| error.to_string())?.0;
    if parent.is_null() {
        return Err("Tauri returned a null Windows HWND".into());
    }
    let class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
    // SAFETY: parent is owned by Tauri; STATIC is a built-in window class and the child
    // lives until its parent is destroyed by Windows.
    let child = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
            0,
            0,
            1,
            1,
            parent,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    if child.is_null() {
        return Err(format!(
            "CreateWindowExW failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Keep the video child behind WebView2 so the transparent overlay controls remain usable.
    unsafe {
        SetWindowPos(child, HWND_BOTTOM, 0, 0, 1, 1, SWP_NOACTIVATE);
    }
    Ok(child as usize)
}

#[cfg(target_os = "windows")]
fn set_surface_rect(
    window: &WebviewWindow,
    handle: usize,
    rect: VideoSurfaceRect,
) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BOTTOM, SW_HIDE, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
        ShowWindow,
    };

    if handle == 0 || rect.width < 0.0 || rect.height < 0.0 {
        return Err("invalid native video surface rectangle".into());
    }
    let hidden = rect.width <= 1.0 || rect.height <= 1.0;
    let x = rect.x.round() as i32;
    let y = rect.y.round() as i32;
    let width = rect.width.round().max(1.0) as i32;
    let height = rect.height.round().max(1.0) as i32;
    window
        .run_on_main_thread(move || {
            // SAFETY: handle is the application-lifetime child HWND created above.
            unsafe {
                if hidden {
                    ShowWindow(handle as *mut std::ffi::c_void, SW_HIDE);
                }
                SetWindowPos(
                    handle as *mut std::ffi::c_void,
                    HWND_BOTTOM,
                    if hidden { 0 } else { x },
                    if hidden { 0 } else { y },
                    if hidden { 1 } else { width },
                    if hidden { 1 } else { height },
                    SWP_NOACTIVATE
                        | if hidden {
                            SWP_HIDEWINDOW
                        } else {
                            SWP_SHOWWINDOW
                        },
                );
            }
        })
        .map_err(|error| error.to_string())
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn create_surface(_window: &WebviewWindow) -> Result<usize, String> {
    Err("native video surface is not implemented for this platform".into())
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn set_surface_rect(
    _window: &WebviewWindow,
    _handle: usize,
    _rect: VideoSurfaceRect,
) -> Result<(), String> {
    Ok(())
}
