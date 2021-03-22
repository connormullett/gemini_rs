extern crate pem;
extern crate rustls;

use std::{
    fs::File,
    io::{self, BufReader, Read},
    net::{SocketAddr, TcpListener},
    path::Path,
    sync::Arc,
};

use rustls::{
    internal::pemfile::certs, AllowAnyAnonymousOrAuthenticatedClient, Certificate, PrivateKey,
    RootCertStore, Session,
};

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

    let client_auth = AllowAnyAnonymousOrAuthenticatedClient::new(client_auth_roots);

    let mut config = rustls::ServerConfig::new(client_auth);

    config.set_persistence(rustls::ServerSessionMemoryCache::new(256));

    let private_key = load_key(Path::new("./key.pem")).unwrap();

    config.set_single_cert(roots, private_key).unwrap();

    Arc::new(config)
}

fn main() {
    let addr = SocketAddr::from(([0, 0, 0, 0], 1965));

    let config = make_config();

    let listener = TcpListener::bind(addr).expect("cant listen on port");

    loop {
        match listener.accept() {
            Ok((mut socket, addr)) => {
                println!("Accepting new connection from {:?}", addr);

                let mut tls_session = rustls::ServerSession::new(&config);
                let _ = tls_session.read_tls(&mut socket);
            }
            _ => {}
        }
    }
}
