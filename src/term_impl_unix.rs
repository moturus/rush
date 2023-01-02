use libc::termios as Termios;

pub(super) struct TermImpl {
    cooked_termios: Termios,
    raw_termios: Termios,
}

impl TermImpl {
    pub(super) fn readline_start(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDOUT_FILENO, libc::TCSANOW, &self.raw_termios);
        }
    }

    pub(super) fn readline_done(&mut self) {
        unsafe {
            libc::tcsetattr(libc::STDOUT_FILENO, libc::TCSANOW, &self.cooked_termios);
        }
    }

    pub(super) fn on_exit(&mut self) {
        self.readline_done(); // Restore termios.
    }

    pub(super) fn new() -> Self {
        let mut cooked_termios: Termios = unsafe { core::mem::zeroed() };
        unsafe {
            libc::tcgetattr(libc::STDOUT_FILENO, &mut cooked_termios);
        }

        let mut raw_termios: Termios = cooked_termios;
        // We do not call 'libc::cfmakeraw(&mut raw_termios)'
        // at the resulting terminal becomes too raw. We need it
        // slightly cooked.

        raw_termios.c_iflag &=
            !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);

        // raw_termios.c_oflag &= !libc::OPOST;

        raw_termios.c_cflag |= libc::CS8;
        raw_termios.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);

        Self {
            cooked_termios,
            raw_termios,
        }
    }
}
