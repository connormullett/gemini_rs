extern crate pem;
extern crate rustls;

#[macro_use]
extern crate log;

use env_logger;
use log::Level;

use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    net::{SocketAddr, TcpListener},
    path::Path,
    sync::Arc,
};

use rustls::{internal::pemfile::certs, Certificate, NoClientAuth, PrivateKey, RootCertStore};

fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
}

fn load_key(path: &Path) -> io::Result<PrivateKey> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut data = String::new();
    let _ = reader.read_to_string(&mut data)?;
    let pem = pem::parse(data);

    Ok(PrivateKey(pem.unwrap().contents))
}

fn make_config() -> Arc<rustls::ServerConfig> {
    let roots = load_certs(Path::new("./cert.pem")).unwrap();
    let mut client_auth_roots = RootCertStore::empty();
    for root in &roots {
        client_auth_roots.add(&root).unwrap();
    }

    let client_auth = NoClientAuth::new();

    let mut config = rustls::ServerConfig::new(client_auth);

    config.set_persistence(rustls::ServerSessionMemoryCache::new(256));

    let private_key = load_key(Path::new("./key.pem")).unwrap();

    config.set_single_cert(roots, private_key).unwrap();

    Arc::new(config)
}

fn main() {
    let port = 1965;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    env_logger::Builder::new().parse_filters("trace").init();

    let config = make_config();

    let listener = TcpListener::bind(addr).expect("cant listen on port");
    log!(Level::Info, "listening on port {}", 1965);

    loop {
        match listener.accept() {
            Ok((mut socket, addr)) => {
                log!(Level::Info, "Accepting new connection from {:?}", addr);
                let mut tls_session = rustls::ServerSession::new(&config);

                let mut buf = Vec::new();
                let mut stream = rustls::Stream::new(&mut tls_session, &mut socket);
                let read_bytes = stream.read(&mut buf);

                log!(Level::Info, "read_bytes {:?}", read_bytes);

                let request = String::from_utf8_lossy(&buf);
                log!(Level::Info, "request {:?}", request);

                let _ = stream.write_all(b"20 text/gemini\r\n#Hello\r\n");
            }

            _ => {}
        }
    }
}
