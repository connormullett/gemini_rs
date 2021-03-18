use std::{
    fs::File,
    io::{self, BufReader},
    net::{SocketAddr, TcpListener},
    path::Path,
    sync::Arc,
};

use rustls::{
    self,
    internal::pemfile::{certs, rsa_private_keys},
    Certificate, NoClientAuth, PrivateKey, Session,
};

fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
}

fn load_keys(path: &Path) -> io::Result<Vec<PrivateKey>> {
    rsa_private_keys(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
}

fn main() {
    let addr = SocketAddr::from(([0, 0, 0, 0], 1965));

    let mut config = rustls::ServerConfig::new(NoClientAuth::new());

    let mut listener = TcpListener::bind(addr).expect("cant listen on port");

    loop {
        match listener.accept() {
            Ok((mut socket, addr)) => {
                println!("Accepting new connection from {:?}", addr);

                let mut tls_session = rustls::ServerSession::new(&Arc::new(config.clone()));
                let request = tls_session.read_tls(&mut socket);
            }
            _ => {}
        }
    }
}
