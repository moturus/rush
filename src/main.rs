#![feature(is_terminal)]

mod exec;
mod line_parser;
mod term;

#[cfg(unix)]
pub mod term_impl_unix;

struct Cleanup {}
impl Drop for Cleanup {
    fn drop(&mut self) {
        term::on_exit();
    }
}

fn main() {
    let is_terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if is_terminal && !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        // Is this even possible? If so, how should we behave?
        eprintln!("rush: stdin is a terminal, but stdiout is not.");
        std::process::exit(1)
    }

    if !is_terminal {
        eprintln!("rush: stdin is not a terminal: not implemented yet.");
        std::process::exit(1)
    }

    let _cleanup = Cleanup {}; // On panic, restore the terminal state.
    let term = term::Term::new();
    let mut parser = line_parser::LineParser::new();

    loop {
        if let Some(commands) = parser.parse_line(term.readline().as_str()) {
            exec::run(commands);
        }
    }
}

pub fn exit(code: i32) -> ! {
    term::on_exit();
    std::process::exit(code)
}
