use std::io::{Read, Stderr, Stdin, Stdout, Write};
use std::sync::atomic::AtomicUsize;
use std::vec::Vec;

#[cfg(unix)]
use crate::term_impl_unix as term_impl;

#[cfg(not(unix))]
mod term_impl {
    pub(super) struct TermImpl {}

    impl TermImpl {
        pub(super) fn readline_start(&mut self) {}
        pub(super) fn readline_done(&mut self) {}

        pub(super) fn on_exit(&mut self) {}

        pub(super) fn new() -> Self {
            Self {}
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum ProcessingMode {
    Normal,
    Escape(Vec<u8>),
    History(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EscapesIn {
    UpArrow,
    DownArrow,
    LeftArrow,
    RightArrow,
    Backspace,
    Delete,
    Home,
    End,
    CtrlC,
}

enum ProcessByteResult {
    Byte(u8), // Normal byte to add;
    Newline,  // Newline: finish processing the line;
    Continue, // Continue processing input;
    Clear,    // Clear current input (e.g. an escape sequence not recognized);
    Escape(EscapesIn),
}

pub struct Term {
    history: Vec<Vec<u8>>,
    mode: ProcessingMode,
    prev_mode: ProcessingMode,
    line: Vec<u8>,
    prev_line: Vec<u8>, // What was typed before an Up arrow was hit.
    line_start: u32,    // Where the input starts after the prompt.
    current_pos: u32,   // Relative to line start.

    term_impl: term_impl::TermImpl,

    escapes_in: std::collections::BTreeMap<&'static [u8], EscapesIn>,

    stdin: Stdin,
    stdout: Stdout,
    stderr: Stderr,

    debug: bool,
}

// Store a pointer to the singleton Term. Don't bother with mutexes,
// as the application is single-threaded for now. If/when it becomes
// multithreaded, we will refactor.
static TERM: AtomicUsize = AtomicUsize::new(0);

pub fn on_exit() {
    let term_addr = TERM.load(std::sync::atomic::Ordering::Relaxed);
    if term_addr == 0 {
        return;
    }

    // Safe because we are single-threaded: see Term::new().
    unsafe {
        let term = (term_addr as *mut Term).as_mut().unwrap();
        term.write("\x1b[ q".as_bytes()); // Reset the cursor.
        term.term_impl.on_exit();
    }
}

impl Term {
    pub fn new() -> &'static mut Self {
        let mut escapes_in: std::collections::BTreeMap<&'static [u8], EscapesIn> =
            std::collections::BTreeMap::new();

        escapes_in.insert(&[0x1b, b'[', b'A'], EscapesIn::UpArrow);
        escapes_in.insert(&[0x1b, b'[', b'B'], EscapesIn::DownArrow);
        escapes_in.insert(&[0x1b, b'[', b'C'], EscapesIn::RightArrow);
        escapes_in.insert(&[0x1b, b'[', b'D'], EscapesIn::LeftArrow);
        escapes_in.insert("\x1b[3~".as_bytes(), EscapesIn::Delete);
        escapes_in.insert("\x1b[1~".as_bytes(), EscapesIn::Home);
        escapes_in.insert("\x1b[7~".as_bytes(), EscapesIn::Home);
        escapes_in.insert("\x1b[H".as_bytes(), EscapesIn::Home);
        escapes_in.insert("\x1b[4~".as_bytes(), EscapesIn::End);
        escapes_in.insert("\x1b[8~".as_bytes(), EscapesIn::End);

        let self_ = Box::leak(Box::new(Self {
            history: vec![],
            mode: ProcessingMode::Normal,
            prev_mode: ProcessingMode::Normal,
            line: vec![],
            prev_line: vec![],
            term_impl: term_impl::TermImpl::new(),
            escapes_in,

            stdin: std::io::stdin(),
            stdout: std::io::stdout(),
            stderr: std::io::stderr(),

            line_start: 0,
            current_pos: 0,

            debug: false,
        }));

        let term_addr = self_ as *mut _ as usize;
        let prev = TERM.swap(term_addr, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(prev, 0);

        self_
    }

    fn process_next_byte(&mut self, c: u8) -> ProcessByteResult {
        match &self.mode {
            ProcessingMode::Normal | ProcessingMode::History(_) => {
                match c {
                    32..=126 => {
                        ProcessByteResult::Byte(c)
                    }
                    128.. => {
                        // Ignore non-ascii bytes for now.
                        ProcessByteResult::Continue
                    }
                    3 => {
                        ProcessByteResult::Escape(EscapesIn::CtrlC)
                    }
                    8 | 127 /* BS */ => {
                        ProcessByteResult::Escape(EscapesIn::Backspace)
                    },
                    13 | 10 /* CR/NL */ => {
                        ProcessByteResult::Newline
                    }
                    0x1b /* ESC */ => {
                        self.prev_mode = self.mode.clone();
                        self.mode = ProcessingMode::Escape(vec![0x1b]);
                        ProcessByteResult::Continue
                    }
                    9 /* TAB */ => {
                        self.debug_log("TAB");
                        ProcessByteResult::Continue
                    }
                    _ => {
                        self.debug_log(format!("unrecognized char: 0x{:x}", c).as_str());
                        self.write(&[7_u8]);  // Beep.
                        ProcessByteResult::Continue
                    }
                }
            }
            ProcessingMode::Escape(v) => {
                let mut candidate_key = v.clone();
                candidate_key.push(c);

                if v.len() == 1 {
                    match c {
                        b'[' => {
                            self.mode = ProcessingMode::Escape(candidate_key);
                            return ProcessByteResult::Continue;
                        }
                        _ => {
                            // There are no recognized keys that start with anything other than "\x1b[".
                            self.debug_log(
                                format!("unknown escape sequence: 0x{:x?}", &candidate_key[0..])
                                    .as_str(),
                            );
                            self.mode = self.prev_mode.clone();
                            return ProcessByteResult::Clear;
                        }
                    }
                }

                match c {
                    b'0'..=b'9' | b';' => {
                        // Continue on numbers and ';'.
                        self.mode = ProcessingMode::Escape(candidate_key);
                        return ProcessByteResult::Continue;
                    }
                    _ => {
                        // Break otherwise.
                    }
                }

                match self.escapes_in.get(&candidate_key[0..]) {
                    Some(val) => {
                        self.mode = self.prev_mode.clone();
                        ProcessByteResult::Escape(*val)
                    }
                    None => {
                        // Not found.
                        self.debug_log(
                            format!("unknown escape sequence: 0x{:x?}", &candidate_key[0..])
                                .as_str(),
                        );
                        self.mode = self.prev_mode.clone();
                        ProcessByteResult::Clear
                    }
                }
            }
        }
    }

    pub fn readline(&mut self) -> String {
        self.term_impl.readline_start();
        self.start_line();

        if !self.history.is_empty() {
            let msg = format!(
                "cmd: {}",
                std::str::from_utf8(self.history.last().as_ref().unwrap()).unwrap()
            );
            self.debug_log(msg.as_str());
        }

        let mut buf: [u8; 16] = [0; 16];

        loop {
            let sz = self.stdin.read(&mut buf).unwrap();
            for idx in 0..sz {
                match self.process_next_byte(buf[idx]) {
                    ProcessByteResult::Byte(c) => {
                        match self.mode {
                            ProcessingMode::Normal => {}
                            ProcessingMode::Escape(_) | ProcessingMode::History(_) => {
                                self.mode = ProcessingMode::Normal;
                                self.show_cursor();
                            }
                        }
                        assert!(self.current_pos <= (self.line.len() as u32));
                        if self.current_pos == (self.line.len() as u32) {
                            // Add to end.
                            self.line.push(c);
                            self.write(&[c]);
                        } else {
                            // Insert.
                            self.line.insert(self.current_pos as usize, c);
                            self.redraw_line();
                            self.write(&[0x1b, b'[', b'1', b'C']); // Move right.
                        }
                        self.current_pos += 1;
                        self.debug_log(format!("got c {}", c).as_str());
                    }
                    ProcessByteResult::Newline => {
                        match self.mode {
                            ProcessingMode::Normal => {}
                            ProcessingMode::Escape(_) | ProcessingMode::History(_) => {
                                self.mode = ProcessingMode::Normal;
                                self.show_cursor();
                            }
                        }
                        let cmd = match std::str::from_utf8(&self.line[..]) {
                            Ok(s) => s.trim(),
                            Err(err) => {
                                eprintln!("\nError: non-utf8 input: {:?}.", err);
                                crate::exit(1);
                            }
                        }
                        .to_owned();
                        if cmd.is_empty() {
                            self.write("\r\n".as_bytes());
                            self.start_line();
                            break;
                        }
                        if self.process_locally(cmd.as_str()) {
                            break;
                        } else {
                            self.write("\r\n".as_bytes());
                            self.term_impl.readline_done();
                            self.maybe_add_to_history(cmd.as_str());
                            return cmd;
                        }
                    }
                    ProcessByteResult::Continue => {}
                    ProcessByteResult::Escape(e) => match e {
                        EscapesIn::UpArrow => match self.mode {
                            ProcessingMode::Normal => {
                                if self.history.len() > 0 {
                                    self.mode = ProcessingMode::History(self.history.len() - 1);
                                    self.show_cursor();
                                    let prev = self.history.last().unwrap().clone();
                                    if self.line == prev {
                                        continue;
                                    }
                                    self.prev_line = self.line.clone();
                                    self.line = prev;
                                    self.current_pos = self.line.len() as u32;
                                    self.redraw_line();
                                } else {
                                    self.beep();
                                }
                            }
                            ProcessingMode::Escape(_) => {
                                panic!("UpArrow: unexpected 'Escape' mode.");
                            }
                            ProcessingMode::History(idx) => {
                                if idx > 0 {
                                    self.mode = ProcessingMode::History(idx - 1);
                                    self.line = self.history[idx - 1].clone();
                                    self.current_pos = self.line.len() as u32;
                                    self.redraw_line();
                                } else {
                                    self.beep();
                                }
                            }
                        },
                        EscapesIn::DownArrow => match self.mode {
                            ProcessingMode::Normal => self.beep(),
                            ProcessingMode::Escape(_) => {
                                panic!("UpArrow: unexpected 'Escape' mode.");
                            }
                            ProcessingMode::History(idx) => {
                                if idx == self.history.len() {
                                    self.beep(); // prev_line
                                } else {
                                    self.mode = ProcessingMode::History(idx + 1);
                                    if idx == (self.history.len() - 1) {
                                        self.line = self.prev_line.clone();
                                    } else {
                                        self.line = self.history[idx + 1].clone();
                                    }
                                    self.current_pos = self.line.len() as u32;
                                    self.redraw_line();
                                }
                            }
                        },
                        EscapesIn::LeftArrow => {
                            if self.current_pos == 0 {
                                self.beep();
                                continue;
                            }
                            self.current_pos -= 1;
                            self.write(&[0x1b, b'[', b'1', b'D']);
                            continue;
                        }
                        EscapesIn::RightArrow => {
                            if self.current_pos >= (self.line.len() as u32) {
                                self.beep();
                                continue;
                            }
                            self.write(&[0x1b, b'[', b'1', b'C']);
                            self.current_pos += 1;
                            continue;
                        }
                        EscapesIn::Backspace => {
                            match self.mode {
                                ProcessingMode::Normal => {}
                                ProcessingMode::Escape(_) | ProcessingMode::History(_) => {
                                    self.mode = ProcessingMode::Normal;
                                    self.show_cursor();
                                }
                            }
                            if self.current_pos > 0 {
                                self.current_pos -= 1;
                                self.line.remove(self.current_pos as usize);
                                self.write(&[0x1b, b'[', b'1', b'D']);
                                self.redraw_line();
                            } else {
                                self.beep();
                            }
                            continue;
                        }
                        EscapesIn::Delete => {
                            match self.mode {
                                ProcessingMode::Normal => {}
                                ProcessingMode::Escape(_) | ProcessingMode::History(_) => {
                                    self.mode = ProcessingMode::Normal;
                                    self.show_cursor();
                                }
                            }
                            if self.current_pos < (self.line.len() as u32) {
                                self.line.remove(self.current_pos as usize);
                                self.redraw_line();
                            } else {
                                self.beep();
                            }
                        }
                        EscapesIn::Home => {
                            if self.current_pos > 0 {
                                self.current_pos = 0;
                                let (row, _) = self.get_cursor_pos();
                                self.move_cursor(row, self.line_start);
                            }
                        }
                        EscapesIn::End => {
                            if self.current_pos < (self.line.len() as u32) {
                                self.current_pos = self.line.len() as u32;
                                let (row, _) = self.get_cursor_pos();
                                self.move_cursor(row, self.line_start + self.current_pos);
                            }
                        }
                        EscapesIn::CtrlC => {
                            match self.mode {
                                ProcessingMode::Normal => {}
                                ProcessingMode::Escape(_) | ProcessingMode::History(_) => {
                                    self.mode = ProcessingMode::Normal;
                                    self.show_cursor();
                                }
                            }
                            self.write("^C\n\r".as_bytes());
                            self.start_line();
                        }
                    },
                    ProcessByteResult::Clear => {
                        self.beep();
                        break;
                    }
                }
            }
        }
    }

    fn beep(&mut self) {
        self.write(&[7_u8]); // Beep.
    }

    fn write(&mut self, bytes: &[u8]) {
        let written = self.stdout.write(bytes).unwrap();
        assert_eq!(written, bytes.len());
        self.stdout.flush().unwrap();
    }

    fn start_line(&mut self) {
        prompt(&mut self.stdout, &mut self.stderr);
        self.line.clear();
        self.prev_line.clear();
        let (_, col) = self.get_cursor_pos();
        self.line_start = col;
        self.current_pos = 0;
        self.mode = ProcessingMode::Normal;
    }

    fn debug_log(&mut self, msg: &str) {
        if !self.debug {
            return;
        }
        let (row, col) = self.get_cursor_pos();
        assert_eq!(col, self.line_start + self.current_pos);

        self.hide_cursor();
        self.move_cursor(1, 1);
        self.write("\x1b[K".as_bytes());
        self.write(format!("\x1b[32m{}:{} | ", row, col).as_bytes());
        self.write(msg.as_bytes());
        self.move_cursor(2, 1);
        self.write("\x1b[K".as_bytes());
        self.write("----------------------\x1b[0m".as_bytes());
        self.move_cursor(row, col);
        self.show_cursor();
    }

    fn hide_cursor(&mut self) {
        self.write("\x1b[?25l".as_bytes());
    }

    fn show_cursor(&mut self) {
        self.write("\x1b[?25h".as_bytes());
        /*
        CSI Ps SP q
          Set cursor style (DECSCUSR, VT520).
            Ps = 0  -> blinking block.
            Ps = 1  -> blinking block (default).
            Ps = 2  -> steady block.
            Ps = 3  -> blinking underline.
            Ps = 4  -> steady underline.
            Ps = 5  -> blinking bar (xterm).
            Ps = 6  -> steady bar (xterm).
        */
        match self.mode {
            ProcessingMode::Normal => self.write("\x1b[5 q".as_bytes()),
            ProcessingMode::Escape(_) => self.write("\x1b[1 q".as_bytes()),
            ProcessingMode::History(_) => self.write("\x1b[2 q".as_bytes()),
        };
    }

    fn move_cursor(&mut self, row: u32, col: u32) {
        if row == 1 && col == 1 {
            self.write("\x1b[H".as_bytes());
            return;
        }
        self.write(format!("\x1b[{};{}H", row, col).as_bytes());
    }

    fn redraw_line(&mut self) {
        let (row, _) = self.get_cursor_pos();
        self.hide_cursor();
        self.move_cursor(row, self.line_start);

        self.write("\x1b[K".as_bytes());

        // Write to stdout instead of self.write() to avoid borrow checker complaints.
        self.stdout.write(&self.line[0..]).unwrap();
        self.stdout.flush().unwrap();

        self.move_cursor(row, self.line_start + self.current_pos);
        self.show_cursor();
    }

    fn try_get_cursor_pos(&mut self) -> Result<(u32, u32), ()> {
        self.write(&[0x1b, b'[', b'6', b'n']); // Query the terminal for cursor position.

        let mut buf = [0; 32];
        let mut offset = 0_usize;
        let mut reading_rows = true;
        let mut row = 0_u32;
        let mut col = 0_u32;
        let mut idx = 2_usize;

        'outer: loop {
            let sz = self.stdin.read(&mut buf[offset..]).unwrap();
            assert!((sz + offset) < buf.len());

            if offset + sz < 2 {
                offset += sz;
                continue;
            }

            if buf[0] != 0x1b {
                return Err(());
            }
            assert_eq!(b'[', buf[1]);

            while idx < (offset + sz) {
                let c = buf[idx];
                if c == b';' {
                    assert!(reading_rows);
                    reading_rows = false;
                    idx += 1;
                    continue;
                }
                if c == b'R' {
                    assert!(!reading_rows);
                    assert_eq!(idx, (offset + sz - 1));
                    break 'outer;
                }

                assert!(c >= b'0');
                assert!(c <= b'9');
                let n = (c - b'0') as u32;

                if reading_rows {
                    row = row * 10 + n;
                } else {
                    col = col * 10 + n;
                }

                idx += 1;
            }

            offset += sz;
        }

        Ok((row, col))
    }

    fn get_cursor_pos(&mut self) -> (u32, u32) {
        loop {
            if let Ok(res) = self.try_get_cursor_pos() {
                return res;
            }
        }
    }

    fn maybe_add_to_history(&mut self, cmd: &str) {
        if self.history.len() == 0 || *self.history.last().unwrap() != cmd.as_bytes() {
            self.history.push(Vec::from(cmd.as_bytes()));
        }
    }

    fn process_locally(&mut self, cmd: &str) -> bool {
        match cmd {
            "clear" => {
                self.write("\x1b[2J".as_bytes()); // Clear screen.
                if self.debug {
                    self.move_cursor(3, 1);
                } else {
                    self.move_cursor(1, 1);
                }
                self.maybe_add_to_history(cmd);
                self.start_line();

                true
            }
            "history" => {
                self.stdout.write("\r\n".as_bytes()).unwrap();

                for line in &self.history {
                    let written = self.stdout.write(line).unwrap();
                    assert_eq!(written, line.len());
                    self.stdout.write("\r\n".as_bytes()).unwrap();
                }
                self.stdout.flush().unwrap();
                self.maybe_add_to_history(cmd);
                self.start_line();

                true
            }
            "--debug" => {
                self.debug = !self.debug;
                self.maybe_add_to_history(cmd);
                self.write("\r\n".as_bytes());
                self.start_line();
                true
            }
            _ => false,
        }
    }
}

fn prompt(stdout: &mut Stdout, stderr: &mut Stderr) {
    stderr.flush().unwrap();
    stdout
        .write(
            format!(
                "\r\x1b[32mrush:\x1b[0m {}$ ",
                std::env::current_dir().unwrap().as_path().to_str().unwrap(),
            )
            .as_bytes(),
        )
        .map(|_| ())
        .unwrap();

    stdout.flush().unwrap();
}
