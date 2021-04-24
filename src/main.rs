use native_tls::{Identity, TlsAcceptor, TlsStream};
use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

#[macro_use]
extern crate log;
use env_logger;

enum RequestError {
    UnexpectedClose,
}

fn read_request<T>(stream: &mut TlsStream<T>) -> Result<Vec<u8>, RequestError>
where
    T: Read + Write,
{
    let mut request = [0; 1026];
    let mut buf = &mut request[..];
    let mut len = 0;

    let _ = loop {
        let bytes_read = if let Ok(read) = stream.read(buf) {
            read
        } else {
            break Err(RequestError::UnexpectedClose);
        };
        len += bytes_read;
        if request[..len].ends_with(b"\r\n") {
            break Ok(());
        } else if bytes_read == 0 {
            break Err(RequestError::UnexpectedClose);
        }
        buf = &mut request[len..];
    }?;

    Ok(request[..len - 2].to_vec())
}

fn handle_client(stream: &mut TlsStream<TcpStream>) {
    let request = match read_request(stream) {
        Ok(value) => value,
        Err(_) => panic!(),
    };

    info!("request {}", String::from_utf8(request).unwrap());

    let out_data = b"20 text/gemini\r\n#Hello\r\n";
    stream.write(out_data).unwrap();
    info!("response 20");
}

fn main() {
    env_logger::Builder::new().parse_filters("trace").init();

    let mut file = File::open("localhost.pfx").unwrap();
    let mut identity = vec![];

    file.read_to_end(&mut identity).unwrap();
    let identity = Identity::from_pkcs12(&identity, "hunter2").unwrap();

    let listener = TcpListener::bind("0.0.0.0:1965").unwrap();
    let acceptor = TlsAcceptor::new(identity).unwrap();
    let acceptor = Arc::new(acceptor);

    info!("listening on port 1965");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("new connection");

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
