use std::{path::Path, process::Stdio};

pub fn run(commands: Vec<Vec<String>>) {
    let mut prev_child = None;
    let mut cmd = "";

    for idx in 0..commands.len() {
        let command = &commands[idx];

        cmd = command[0].as_str();
        let args = &command[1..];
        match cmd {
            "cd" => {
                if args.len() != 1 {
                    println!("cd: must have a single argument.");
                    prev_child = None;
                    continue;
                }
                let new_dir = args[0].as_str();
                let root = Path::new(new_dir);
                if let Err(e) = std::env::set_current_dir(&root) {
                    println!("{}", e);
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

                let stderr = if idx < (commands.len() - 1) {
                    Stdio::piped()
                } else {
                    Stdio::inherit()
                };

                let output = std::process::Command::new(command)
                    .args(args)
                    .stdin(stdin)
                    .stdout(stdout)
                    .stderr(stderr)
                    .spawn();

                match output {
                    Ok(output) => {
                        prev_child = Some(output);
                    }
                    Err(e) => {
                        prev_child = None;
                        match e.kind() {
                            std::io::ErrorKind::InvalidFilename => {
                                println!("{}: command not found.", cmd)
                            }
                            _ => println!("Command [{}] failed with error: [{}].", command, e),
                        }
                    }
                };
            }
        }
    }

    if let Some(mut last) = prev_child {
        match last.wait() {
            Ok(status) => {
                if !status.success() {
                    if let Some(code) = status.code() {
                        println!("[{}] exited with status {:?}", cmd, code);
                    }
                }
            }
            Err(err) => {
                println!("{:?}", err);
            }
        }
    }
}
