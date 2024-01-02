use std::{io::Read, io::Write, net::TcpStream, time::Duration};

pub struct RemoteRelay {
    remote_conn: TcpStream,
}

impl RemoteRelay {
    pub fn run(&mut self) -> ! {
        todo!()
        /*
        assert_eq!(self.state, State::LocalInput);

        let len: u64 = commands.as_bytes().len() as u64;
        let buf: &[u8] = unsafe { core::slice::from_raw_parts(&len as *const _ as *const u8, 8) };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
        self.remote_conn.write_all(buf).unwrap();

        self.remote_conn.write_all(commands.as_bytes()).unwrap();
        self.remote_conn.flush().unwrap();

        self.state = State::RemoteOutput;

        let mut buf = [0_u8; 80];
        let mut incoming = 0_i64;
        let mut incoming_buf =
            unsafe { core::slice::from_raw_parts_mut(&mut incoming as *mut _ as *mut u8, 8) };

        loop {
            self.remote_conn.read_exact(&mut incoming_buf).unwrap();
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::AcqRel);
            if incoming == -1 {
                // Done.
                break;
            }
            if incoming < 1 {
                eprintln!("Unrecognized remote info.");
                return Err(());
            }

            let mut consumed = 0_i64;
            while consumed < incoming {
                if let Ok(sz) = self.remote_conn.read(&mut buf) {
                    if sz > 0 {
                        stdout().write_all(&buf[0..sz]).unwrap();
                        stdout().flush().unwrap();
                        consumed += sz as i64;
                        continue;
                    } else {
                        eprintln!("Unexpected remote EOF.");
                        return Err(());
                    }
                }
            }
        }

        self.state = State::LocalInput;
        Ok(())
        */
    }
}

pub fn connect_to(host_port: &str) -> RemoteRelay {
    use std::net::ToSocketAddrs;

    let mut addresses = vec![];
    match host_port.to_socket_addrs() {
        Ok(addrs) => {
            for addr in addrs {
                addresses.push(addr);
            }
        }
        Err(_) => crate::print_usage_and_exit(1),
    }

    if addresses.len() != 1 {
        crate::print_usage_and_exit(1);
    }
    let addr = addresses[0];

    let mut remote_conn = match TcpStream::connect_timeout(&addr, Duration::new(1, 0)) {
        Ok(stream) => stream,
        Err(err) => {
            eprintln!("rush: error connecting to {}: {:?}.", host_port, err);
            std::process::exit(1);
        }
    };

    // Handshake.
    remote_conn.set_nodelay(true).unwrap();
    if let Err(_) = remote_conn.write_all(crate::RUSH_HANDSHAKE.as_bytes()) {
        eprintln!("rush: handshake failed (1).");
        std::process::exit(1);
    }
    remote_conn.flush().unwrap();
    let mut buf = [0_u8; crate::RUSH_HANDSHAKE.len()];
    if let Err(_) = remote_conn.read_exact(&mut buf) {
        eprintln!("rush: handshake failed (2).");
        std::process::exit(1);
    }
    if buf != crate::RUSH_HANDSHAKE.as_bytes() {
        eprintln!("rush: handshake failed (3).");
        std::process::exit(1);
    }

    println!("rush: connected to {}", host_port);
    RemoteRelay { remote_conn }
}
