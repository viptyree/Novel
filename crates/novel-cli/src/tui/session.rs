//! A single alternate-screen lifetime, shared by menus, readers and editors.
use super::*;
use std::ops::{Deref, DerefMut};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, MutexGuard,
};

static SCREEN: Mutex<Option<Screen>> = Mutex::new(None);
static PERSISTENT: AtomicBool = AtomicBool::new(false);

pub(super) fn restore() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

pub(super) fn suspend() {
    if let Ok(mut screen) = SCREEN.lock() {
        screen.take();
    }
    restore();
}

pub(super) struct Lifetime(bool);
impl Lifetime {
    pub(super) fn new(enabled: bool) -> Self {
        PERSISTENT.store(enabled, Ordering::Relaxed);
        Self(enabled)
    }
}
impl Drop for Lifetime {
    fn drop(&mut self) {
        PERSISTENT.store(false, Ordering::Relaxed);
        if self.0 {
            suspend();
        }
    }
}

pub(super) struct SharedScreen(MutexGuard<'static, Option<Screen>>);
impl Deref for SharedScreen {
    type Target = Screen;
    fn deref(&self) -> &Screen {
        self.0.as_ref().expect("screen lease initialized")
    }
}
impl DerefMut for SharedScreen {
    fn deref_mut(&mut self) -> &mut Screen {
        self.0.as_mut().expect("screen lease initialized")
    }
}
pub(super) struct Session {
    pub(super) terminal: SharedScreen,
}
impl Session {
    pub(super) fn new() -> Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err("交互界面需要 TTY；SSH 请分配终端，或使用标准命令".into());
        }
        let mut screen = SCREEN.lock().map_err(|_| "终端状态不可用")?;
        if screen.is_none() {
            terminal::enable_raw_mode().map_err(|e| e.to_string())?;
            if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste) {
                restore();
                return Err(e.to_string());
            }
            let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| {
                restore();
                e.to_string()
            })?;
            terminal.clear().map_err(|e| {
                restore();
                e.to_string()
            })?;
            *screen = Some(terminal);
        }
        Ok(Self {
            terminal: SharedScreen(screen),
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !PERSISTENT.load(Ordering::Relaxed) {
            self.terminal.0.take();
            restore();
        }
    }
}
