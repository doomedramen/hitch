use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Stdout};

pub struct TerminalGuard {
    stdout: Stdout,
    restored: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self {
            stdout,
            restored: false,
        })
    }

    pub fn stdout_mut(&mut self) -> &mut Stdout {
        &mut self.stdout
    }

    pub fn restore(&mut self) {
        if self.restored {
            return;
        }
        let _ = execute!(self.stdout, LeaveAlternateScreen, DisableMouseCapture);
        let _ = disable_raw_mode();
        self.restored = true;
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}
