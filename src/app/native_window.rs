//! Window operations used by tray callbacks must not depend on another egui frame.
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
    },
    UI::WindowsAndMessaging::{
        IsIconic, IsWindow, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
    },
};

/// A borrowed HWND. The main window owns its lifetime; callbacks check it before
/// use. The integer representation allows the Send + Sync tray handler closures.
#[derive(Clone, Copy)]
pub(super) struct NativeWindow(isize);

impl NativeWindow {
    pub(super) fn from_context(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        match cc.window_handle()?.as_raw() {
            RawWindowHandle::Win32(handle) => Ok(Self(handle.hwnd.get())),
            _ => anyhow::bail!("Нет дескриптора окна Windows для работы с треем"),
        }
    }

    fn hwnd(self) -> HWND {
        self.0 as HWND
    }

    pub(super) fn dark_titlebar(self) {
        let dark: i32 = 1;
        let caption: u32 = 43 | (45 << 8) | (48 << 16);
        let text: u32 = 223 | (225 << 8) | (229 << 16);
        // Windows 10 supports the dark mode flag. Explicit caption/text colors
        // are supported on Windows 11; older systems safely ignore these calls.
        unsafe {
            DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                (&dark as *const i32).cast(),
                size_of::<i32>() as u32,
            );
            DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_CAPTION_COLOR as u32,
                (&caption as *const u32).cast(),
                size_of::<u32>() as u32,
            );
            DwmSetWindowAttribute(
                self.hwnd(),
                DWMWA_TEXT_COLOR as u32,
                (&text as *const u32).cast(),
                size_of::<u32>() as u32,
            );
        }
    }

    pub(super) fn restore(self) {
        unsafe {
            if IsWindow(self.hwnd()) == 0 {
                return;
            }
            self.dark_titlebar();
            let command = if IsIconic(self.hwnd()) != 0 {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            ShowWindow(self.hwnd(), command);
            SetForegroundWindow(self.hwnd());
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn visible(self) -> bool {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd()) != 0 }
    }

    #[cfg(debug_assertions)]
    pub(super) fn request_close(self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        unsafe {
            PostMessageW(self.hwnd(), WM_CLOSE, 0, 0);
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn dark_mode_enabled(self) -> bool {
        use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
        let mut dark = 0_i32;
        let mut caption = 0_u32;
        unsafe {
            let dark_set = DwmGetWindowAttribute(
                self.hwnd(),
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                (&mut dark as *mut i32).cast(),
                size_of::<i32>() as u32,
            ) >= 0
                && dark != 0;
            let caption_supported = DwmGetWindowAttribute(
                self.hwnd(),
                DWMWA_CAPTION_COLOR as u32,
                (&mut caption as *mut u32).cast(),
                size_of::<u32>() as u32,
            ) >= 0;
            dark_set && (!caption_supported || caption == 43 | (45 << 8) | (48 << 16))
        }
    }
}
