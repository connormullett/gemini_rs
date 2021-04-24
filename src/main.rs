use native_tls::{Identity, TlsAcceptor, TlsStream};
use std::fs::File;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

#[macro_use]
extern crate log;

use env_logger;
use log::Level;

fn handle_client(stream: &mut TlsStream<TcpStream>) {
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf).unwrap();
    let buf = String::from_utf8_lossy(&buf);
    log!(Level::Info, "data :: {}", buf);
}

fn main() {
    env_logger::Builder::new().parse_filters("info").init();

    let mut file = File::open("localhost.pfx").unwrap();
    let mut identity = vec![];

    file.read_to_end(&mut identity).unwrap();
    let identity = Identity::from_pkcs12(&identity, "hunter2").unwrap();

    let listener = TcpListener::bind("0.0.0.0:1965").unwrap();
    let acceptor = TlsAcceptor::new(identity).unwrap();
    let acceptor = Arc::new(acceptor);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let acceptor = acceptor.clone();
                thread::spawn(move || {
                    let mut stream = acceptor.accept(stream).unwrap();
                    handle_client(&mut stream);
                });
            }
            Err(e) => {
                warn!("{}", e.to_string())
            }
        }
    }
}
