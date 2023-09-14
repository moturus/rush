use std::{
    fs::File,
    io::{Read, Write},
    process::ChildStdout,
};

pub struct Redirect {
    stdout_redirect: File,
}

impl Redirect {
    pub fn consume_stdout(&mut self, child_stdout: &mut ChildStdout) {
        let mut bytes = vec![];
        child_stdout.read_to_end(&mut bytes).ok();
        self.stdout_redirect.write_all(&bytes).ok();
    }
}

// Parse arguments, find stdiout redirects like "> file" and ">> file".
// TODO: add stdio and stderr redirects.
pub fn parse_args(args: &[String]) -> Result<(Vec<String>, Option<Redirect>), ()> {
    let mut args_out = Vec::with_capacity(args.len());
    let mut op = None;
    let mut redirect_fname = None;
    for arg in args {
        if op.is_some() && redirect_fname.is_none() {
            redirect_fname = Some(arg.clone());
            continue;
        }
        if arg == ">" || arg == ">>" {
            if op.is_some() {
                eprintln!("rush: syntax error: double redirect.");
                return Err(());
            }
            op = Some(arg.clone());
            continue;
        }
        args_out.push(arg.clone());
    }

    let mut redirect = None;
    if op.is_some() {
        if redirect_fname.is_none() {
            eprintln!("rush: syntax error: a redirect without a filename.");
            return Err(());
        }

        let op = op.unwrap();
        let fname = redirect_fname.unwrap();

        let file = if op == ">" {
            std::fs::File::create(fname.clone())
        } else {
            std::fs::OpenOptions::new().append(true).open(fname.clone())
        };

        if file.is_err() {
            eprintln!("rush: can't open file '{}'.", fname);
            return Err(());
        }

        redirect = Some(Redirect {
            stdout_redirect: file.unwrap(),
        });
    }

    Ok((args_out, redirect))
}