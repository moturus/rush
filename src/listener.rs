use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    process::Child,
    sync::{atomic::*, Arc},
    time::Duration,
};

// Intercept Ctrl+C ourselves if the OS does not do it for us.
fn input_listener() {
    loop {
        let mut input = [0_u8; 16];
        let sz = std::io::stdin().read(&mut input).unwrap();
        for b in &input[0..sz] {
            if *b == 3 {
                println!("\nrush: caught ^C: exiting.");
                std::process::exit(0);
            }
        }
    }
}

pub fn run(port: u16) -> ! {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
    let listener = if let Ok(listener) = std::net::TcpListener::bind(addr) {
        listener
    } else {
        eprintln!("rush: TcpListener.bind('0.0.0.0:{}') failed.", port);
        std::process::exit(1);
    };

    println!("rush server: listening on 0.0.0.0:{}\n", port);

    std::thread::spawn(move || input_listener());

    for tcp_stream in listener.incoming() {
        handle_connection(tcp_stream);
    }

    unreachable!()
}

fn handle_connection(maybe_stream: std::io::Result<TcpStream>) {
    match maybe_stream {
        Ok(stream) => {
            let _ = std::thread::spawn(|| {
                server_thread(stream);
            });
        }
        Err(error) => {
            eprintln!("rush: bad connection: {:?}.", error);
            return;
        }
    }
}

fn spawn_shell() -> Child {
    let self_cmd = std::env::args().next().unwrap();
    let mut command = std::process::Command::new(self_cmd.as_str());
    command.arg("-i");
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    command.current_dir("/");

    match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("rush: error spawning sh: {:?}.", err);
            std::process::exit(1);
        }
    }
}

fn server_thread(mut client: TcpStream) {
    let mut buf = [0_u8; crate::RUSH_HANDSHAKE.len()];
    if let Err(_) = client.read_exact(&mut buf) {
        eprintln!("rush: handshake failed (1).");
        return;
    }
    if buf != crate::RUSH_HANDSHAKE.as_bytes() {
        eprintln!("rush: handshake failed (2).");
        return;
    }
    if let Err(_) = client.write_all(crate::RUSH_HANDSHAKE.as_bytes()) {
        eprintln!("rush: handshake failed (3).");
        return;
    }
    let _ = client.flush();

    let remote_addr = if let Ok(addr) = client.peer_addr() {
        addr
    } else {
        return;
    };
    println!("rush: new connection from {:?}.", remote_addr);

    let mut shell = spawn_shell();
    client.set_nodelay(true).unwrap();
    let exit_notifier = Arc::new(AtomicBool::new(false));

    // stdout
    let exit2 = exit_notifier.clone();
    let mut local_stdout = shell.stdout.take().unwrap();
    let mut remote_stdout = client.try_clone().unwrap();
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = [0_u8; 80];
        while !exit2.load(Ordering::Relaxed) {
            if let Ok(sz) = local_stdout.read(&mut buf) {
                if sz > 0 {
                    let sz_u64 = sz as u64;
                    let sz_buf: &[u8] =
                        unsafe { core::slice::from_raw_parts(&sz_u64 as *const _ as *const u8, 8) };
                    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
                    remote_stdout.write_all(sz_buf).unwrap();
                    remote_stdout.write_all(&buf[0..sz]).unwrap();
                }
            } else {
                break;
            }
        }
        // Signal the end of this session.
        let cmd = -1_i64;
        let cmd_buf: &[u8] =
            unsafe { core::slice::from_raw_parts(&cmd as *const _ as *const u8, 8) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
        let _ = remote_stdout.write_all(cmd_buf);
    });

    // stderr
    let exit3 = exit_notifier.clone();
    let mut local_stderr = shell.stderr.take().unwrap();
    let mut remote_stderr = client.try_clone().unwrap();
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = [0_u8; 80];
        while !exit3.load(Ordering::Relaxed) {
            if let Ok(sz) = local_stderr.read(&mut buf) {
                if sz > 0 {
                    let sz_u64 = sz as u64;
                    let sz_buf: &[u8] =
                        unsafe { core::slice::from_raw_parts(&sz_u64 as *const _ as *const u8, 8) };
                    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
                    remote_stderr.write_all(sz_buf).unwrap();
                    remote_stderr.write_all(&buf[0..sz]).unwrap();
                }
            } else {
                break;
            }
        }
    });

    // stdin
    let mut local_stdin = shell.stdin.take().unwrap();
    let mut remote_stdin = client.try_clone().unwrap();
    let stdin_thread = std::thread::spawn(move || {
        remote_stdin
            .set_read_timeout(Some(Duration::new(0, 100_000_000)))
            .unwrap();
        'outer: loop {
            match shell.try_wait() {
                Ok(Some(_)) => {
                    break;
                }
                Ok(None) => {}
                Err(err) => {
                    panic!("{:?}", err);
                }
            }
            let mut buf = [0_u8; 80];
            let mut incoming = 0_i64;
            let mut incoming_buf =
                unsafe { core::slice::from_raw_parts_mut(&mut incoming as *mut _ as *mut u8, 8) };
            if let Err(_) = client.read_exact(&mut incoming_buf) {
                break;
            }
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
            if incoming == -1 {
                // Done.
                break;
            }
            if incoming < 1 {
                eprintln!("Unrecognized remote info.");
                break;
            }

            let mut consumed = 0_i64;
            let mut commands = String::new();
            while consumed < incoming {
                if let Ok(sz) = remote_stdin.read(&mut buf) {
                    if sz > 0 {
                        commands += core::str::from_utf8(&buf[0..sz]).unwrap();
                        consumed += sz as i64;
                        continue;
                    } else {
                        eprintln!("Unexpected remote EOF.");
                        break 'outer;
                    }
                }
            }

            #[cfg(debug_assertions)]
            println!("{}", commands);

            if local_stdin.write_all(commands.as_bytes()).is_err() {
                break;
            }
            if local_stdin.write_all(b"\n").is_err() {
                break;
            }
            local_stdin.flush().unwrap();
        }
        let _ = shell.kill();
        let _ = client.shutdown(std::net::Shutdown::Both);
        let _ = shell.wait().unwrap();
    });

    // println!("server: done: waiting");
    // let _ = shell.wait();
    // println!("server: done: waiting 2");
    stdin_thread.join().unwrap();
    exit_notifier.store(true, Ordering::Release);
    stdout_thread.join().unwrap();
    stderr_thread.join().unwrap();
    println!("rush: connection from {:?} closed.", remote_addr);
}
