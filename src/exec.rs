use std::{path::Path, process::Stdio};

fn is_var(token: &str) -> bool {
    if token.len() == 0 || token.len() != token.trim().len() {
        return false;
    }

    if !token.is_ascii() {
        return false;
    }

    let first: char = token.as_bytes()[0].into();
    if !first.is_alphabetic() {
        return false;
    }

    for b in &token.as_bytes()[1..] {
        let c: char = (*b).into();
        if !c.is_alphanumeric() {
            return false;
        }
    }

    true
}

fn take_env(command: &[String]) -> Option<(&str, &str)> {
    if command.is_empty() {
        return None;
    }

    let cmd = command[0].as_str().trim();
    if let Some((k, v)) = cmd.split_once('=') {
        if is_var(k) {
            Some((k, v))
        } else {
            None
        }
    } else {
        None
    }
}

fn apply_global_env(env: &Vec<(&str, &str)>) {
    for (k, v) in env {
        std::env::set_var(k, v);
    }
}

fn process_vars(tokens: &[String], _env: &Vec<(&str, &str)>, args: &Vec<String>) -> Vec<String> {
    // We should do a proper language interpreter with AST later.
    // For now we have something simple to bootstrap things.

    let mut result = Vec::new();
    for token in tokens {
        if token.as_str() == "$@" {
            for arg in &args.as_slice()[1..] {
                result.push(arg.clone());
            }
        } else {
            result.push(token.clone());
        }
    }

    result
}

pub fn run(commands: Vec<Vec<String>>, global: bool, args: &Vec<String>) -> Result<(), i32> {
    let mut prev_child = None;
    let mut cmd = None;

    if commands.len() > 1 {
        todo!("piping needs better stdio treatment");
    }

    for idx in 0..commands.len() {
        let mut command = commands[idx].as_slice();

        // We should do a proper language interpreter with AST later.
        // For now we have something simple to bootstrap things.

        // Process commands like `A=B do_something`.
        let mut env: Vec<(&str, &str)> = vec![];
        while let Some(k_v) = take_env(command) {
            env.push(k_v);
            command = &command[1..];
        }

        if command.is_empty() {
            if global {
                if idx == 0 && commands.len() == 1 {
                    apply_global_env(&env);
                } else {
                    println!("Error: cannot set global environment variable in a subcommand.");
                    return Err(-1);
                }
            }
            continue;
        }

        // Process inline vars.
        let command = process_vars(command, &env, args);
        if command.len() == 0 {
            continue;
        }

        cmd = Some(command[0].clone());
        let args = &command[1..];
        match cmd.as_ref().unwrap().as_str() {
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
                    .envs(env.into_iter())
                    .spawn();

                match output {
                    Ok(output) => {
                        prev_child = Some(output);
                    }
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::InvalidFilename => {
                            println!("{}: command not found.", cmd.unwrap());
                            return Err(-1);
                        }
                        _ => {
                            println!("Command [{}] failed with error: [{}].", command, e);
                            return Err(-1);
                        }
                    },
                };
            }
        }
    }

    if let Some(mut last) = prev_child {
        match last.wait() {
            Ok(status) => {
                if !status.success() {
                    if let Some(code) = status.code() {
                        println!("[{}] exited with status {:?}", cmd.unwrap(), code);
                        Err(code)
                    } else {
                        Err(-1)
                    }
                } else {
                    Ok(())
                }
            }
            Err(err) => {
                println!("{:?}", err);
                Err(-1)
            }
        }
    } else {
        Ok(())
    }
}

pub fn run_script(fname: &str, args: Vec<String>, global: bool) -> Result<(), i32> {
    let script = {
        match std::fs::read_to_string(std::path::Path::new(fname)) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("Error reading '{}': {:?}", fname, err);
                std::process::exit(1);
            }
        }
    };

    let mut parser = crate::line_parser::LineParser::new();
    for line in script.lines() {
        let line = line.trim();
        if line.is_empty() || line.as_bytes()[0] == b'#' {
            continue;
        }
        if let Some(commands) = parser.parse_line(line) {
            if let Err(err) = run(commands, global, &args) {
                return Err(err);
            }
        }
    }

    Ok(())
}
