//! Bringing the window forward when a second launch hands its request over, without ever stealing the
//! foreground from a full-screen app. Spec: App/Interop/ForegroundWindow.cs and
//! Core/Services/Foreground/ForegroundRaiseRules.cs (here `bb_core::rules`).
//!
//! The hand-off defeats the foreground lock on purpose (the sending copy grants its right to the
//! copy doing the work), so it has to ask before using it. The app is the registered handler for
//! `Directory` and `Drive`, so *anything* that shell-opens a folder starts a second copy, and the
//! running copy once landed on top of full-screen video with no pattern the user could see, because
//! the trigger belonged to another process. When the rules say the user is busy, the window flashes
//! its taskbar button and does not even un-minimize.

use bb_core::rules::{ForegroundRaiseRules, RaiseAction, UserNotificationState};

/// What the raise needs to know about the desktop. A trait so the decision can be driven without one.
pub trait ForegroundProbe {
    /// The window that has the foreground, if any.
    fn foreground_window(&self) -> Option<isize>;
    fn process_of(&self, window: isize) -> u32;
    fn notification_state(&self) -> UserNotificationState;
    fn covers_its_monitor(&self, window: isize) -> bool;
}

/// What a raise does to a window.
pub trait WindowActions {
    /// Restores the window and takes the foreground.
    fn bring_to_front(&self, window: isize);
    /// Flashes the caption and taskbar button until the window comes to the foreground: the signal
    /// the window manager raises by itself when it refuses a `SetForegroundWindow`.
    fn flash(&self, window: isize);
}

pub fn decide(probe: &dyn ForegroundProbe, own: isize, own_process: u32) -> RaiseAction {
    let Some(foreground) = probe.foreground_window() else {
        return RaiseAction::Raise;
    };
    let ours = foreground == own || probe.process_of(foreground) == own_process;
    ForegroundRaiseRules::decide(
        probe.notification_state(),
        ours,
        // Only consulted when the state says "a Store app", so a plain window never pays for it.
        probe.covers_its_monitor(foreground),
    )
}

/// Raises `window` unless the rules say otherwise; returns what was done.
pub fn raise_with(
    probe: &dyn ForegroundProbe,
    actions: &dyn WindowActions,
    window: isize,
    own_process: u32,
) -> RaiseAction {
    let action = decide(probe, window, own_process);
    match action {
        RaiseAction::Flash => actions.flash(window),
        RaiseAction::Raise => actions.bring_to_front(window),
    }
    action
}

/// The real desktop.
pub struct SystemDesktop;

/// Raises (or flashes) the window `hwnd`.
pub fn raise(hwnd: isize) -> RaiseAction {
    raise_with(&SystemDesktop, &SystemDesktop, hwnd, std::process::id())
}

/// Lets the process serving `pipe` take the foreground. Called by the copy that was just started by
/// a double-click (it holds the foreground rights) before it hands its request to the one running.
pub fn grant_to_server_of<H: std::os::windows::io::AsRawHandle>(pipe: &H) {
    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    let mut process_id = 0u32;
    // SAFETY: `pipe` is a client pipe handle owned by the caller; `process_id` is a valid out pointer.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut process_id) } != 0
        && process_id != 0
    {
        // SAFETY: a plain call with a process id.
        unsafe { AllowSetForegroundWindow(process_id) };
    }
}

mod win32 {
    use bb_core::rules::UserNotificationState;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FLASHW_ALL, FLASHW_TIMERNOFG, FLASHWINFO, FlashWindowEx, GetForegroundWindow,
        GetWindowRect, GetWindowThreadProcessId, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    use super::{ForegroundProbe, SystemDesktop, WindowActions};

    fn hwnd(window: isize) -> HWND {
        window as HWND
    }

    impl ForegroundProbe for SystemDesktop {
        fn foreground_window(&self) -> Option<isize> {
            // SAFETY: no arguments.
            let w = unsafe { GetForegroundWindow() };
            if w.is_null() { None } else { Some(w as isize) }
        }

        fn process_of(&self, window: isize) -> u32 {
            let mut id = 0u32;
            // SAFETY: a valid out pointer.
            unsafe { GetWindowThreadProcessId(hwnd(window), &mut id) };
            id
        }

        fn notification_state(&self) -> UserNotificationState {
            let mut state = 0i32;
            // SAFETY: a valid out pointer.
            let hr = unsafe { SHQueryUserNotificationState(&mut state) };
            if hr == 0 {
                UserNotificationState::from_i32(state)
            } else {
                UserNotificationState::Unknown
            }
        }

        fn covers_its_monitor(&self, window: isize) -> bool {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: a valid out pointer.
            if unsafe { GetWindowRect(hwnd(window), &mut rect) } == 0 {
                return false;
            }
            // SAFETY: plain calls; the monitor handle is checked.
            let monitor = unsafe { MonitorFromWindow(hwnd(window), MONITOR_DEFAULTTONEAREST) };
            if monitor.is_null() {
                return false;
            }
            // SAFETY: a zeroed struct with its size set, as the call requires.
            let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            // SAFETY: `info` is initialised.
            if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
                return false;
            }
            let m = info.rcMonitor;
            rect.left <= m.left
                && rect.top <= m.top
                && rect.right >= m.right
                && rect.bottom >= m.bottom
        }
    }

    impl WindowActions for SystemDesktop {
        fn bring_to_front(&self, window: isize) {
            // Restore through the OS: a window minimized by the shell can disagree with the toolkit's
            // own idea of its state, and SetForegroundWindow on a still-minimized window does nothing.
            // SAFETY: plain calls on a window handle.
            unsafe {
                ShowWindow(hwnd(window), SW_RESTORE);
                SetForegroundWindow(hwnd(window));
            }
        }

        fn flash(&self, window: isize) {
            let info = FLASHWINFO {
                cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
                hwnd: hwnd(window),
                dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
                uCount: 0,
                dwTimeout: 0,
            };
            // SAFETY: `info` is fully initialised.
            unsafe { FlashWindowEx(&info) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Desktop {
        foreground: Option<isize>,
        process: u32,
        state: UserNotificationState,
        covers: bool,
    }

    impl ForegroundProbe for Desktop {
        fn foreground_window(&self) -> Option<isize> {
            self.foreground
        }
        fn process_of(&self, _: isize) -> u32 {
            self.process
        }
        fn notification_state(&self) -> UserNotificationState {
            self.state
        }
        fn covers_its_monitor(&self, _: isize) -> bool {
            self.covers
        }
    }

    #[derive(Default)]
    struct Recorder(RefCell<Vec<&'static str>>);

    impl WindowActions for Recorder {
        fn bring_to_front(&self, _: isize) {
            self.0.borrow_mut().push("front");
        }
        fn flash(&self, _: isize) {
            self.0.borrow_mut().push("flash");
        }
    }

    fn desktop(state: UserNotificationState, covers: bool) -> Desktop {
        Desktop {
            foreground: Some(99),
            process: 7,
            state,
            covers,
        }
    }

    #[test]
    fn a_full_screen_game_gets_a_flash_and_no_raise() {
        for state in [
            UserNotificationState::Busy,
            UserNotificationState::RunningD3DFullScreen,
            UserNotificationState::PresentationMode,
        ] {
            let actions = Recorder::default();
            let done = raise_with(&desktop(state, false), &actions, 1, 42);
            assert_eq!(RaiseAction::Flash, done);
            assert_eq!(vec!["flash"], *actions.0.borrow());
        }
    }

    #[test]
    fn a_full_screen_store_app_is_told_apart_by_covering_its_monitor() {
        let actions = Recorder::default();
        assert_eq!(
            RaiseAction::Flash,
            raise_with(&desktop(UserNotificationState::App, true), &actions, 1, 42)
        );
        assert_eq!(
            RaiseAction::Raise,
            raise_with(&desktop(UserNotificationState::App, false), &actions, 1, 42)
        );
    }

    #[test]
    fn nothing_in_the_way_raises() {
        let actions = Recorder::default();
        raise_with(
            &desktop(UserNotificationState::AcceptsNotifications, false),
            &actions,
            1,
            42,
        );
        assert_eq!(vec!["front"], *actions.0.borrow());
    }

    #[test]
    fn when_our_own_window_is_in_front_there_is_nothing_to_interrupt() {
        let mut d = desktop(UserNotificationState::Busy, false);
        d.process = 42;
        assert_eq!(RaiseAction::Raise, decide(&d, 1, 42));
        d.process = 7;
        d.foreground = Some(1);
        assert_eq!(RaiseAction::Raise, decide(&d, 1, 42));
    }

    #[test]
    fn no_foreground_window_and_an_unreadable_state_both_lean_towards_raising() {
        let mut d = desktop(UserNotificationState::Busy, false);
        d.foreground = None;
        assert_eq!(RaiseAction::Raise, decide(&d, 1, 42));
        assert_eq!(
            RaiseAction::Raise,
            decide(&desktop(UserNotificationState::Unknown, false), 1, 42)
        );
    }
}
