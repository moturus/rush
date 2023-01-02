use std::{path::Path, process::Stdio};

pub fn run(commands: Vec<Vec<String>>) {
    let mut prev_child = None;

    for idx in 0..commands.len() {
        let command = &commands[idx];

        let cmd = command[0].as_str();
        let args = &command[1..];
        match cmd {
            "cd" => {
                if args.len() != 1 {
                    eprintln!("cd: must have a single argument.");
                    prev_child = None;
                    continue;
                }
                let new_dir = args[0].as_str();
                let root = Path::new(new_dir);
                if let Err(e) = std::env::set_current_dir(&root) {
                    eprintln!("{}", e);
                }

                prev_child = None;
            }
            "exit" | "quit" => crate::exit(0),
            command => {
                let stdin = prev_child.map_or(Stdio::inherit(), |output: std::process::Child| {
                    Stdio::from(output.stdout.unwrap())
                });

                let stdout = if idx < (commands.len() - 1) {
                    Stdio::piped()
                } else {
                    Stdio::inherit()
                };

                let output = std::process::Command::new(command)
                    .args(args)
                    .stdin(stdin)
                    .stdout(stdout)
                    .spawn();

                match output {
                    Ok(output) => {
                        prev_child = Some(output);
                    }
                    Err(e) => {
                        prev_child = None;
                        eprintln!("{}", e);
                    }
                };
            }
        }
    }

    if let Some(mut last) = prev_child {
        match last.wait() {
            Ok(_status) => {
                /*
                if !status.success() {
                    eprintln!("wait() failed: {:?}", status.code());
                }
                */
            }
            Err(err) => {
                eprintln!("{:?}", err);
            }
        }
    }
}
