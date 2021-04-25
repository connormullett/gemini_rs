use native_tls::{Identity, TlsAcceptor, TlsStream};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::{
    fs,
    io::{Read, Write},
};
use std::{fs::File, path::PathBuf};
use url::Url;

#[macro_use]
extern crate log;
use env_logger;

#[derive(Debug)]
enum RequestError {
    UnexpectedClose,
    UrlParseError,
    IoReadError,
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

fn parse_url(request: String) -> Result<Url, RequestError> {
    let url = Url::parse(&request).map_err(|_| RequestError::UrlParseError)?;
    Ok(url)
}

fn read_file(path: &PathBuf) -> Result<String, RequestError> {
    let content = fs::read_to_string(path).map_err(|error| {
        error!("{}", error);
        RequestError::IoReadError
    })?;
    Ok(content)
}

fn build_response(url: Url) -> Result<String, RequestError> {
    let path_str = PathBuf::from(url.path());
    let mut path = PathBuf::from("content-root");
    path.push(path_str);

    if path.is_file() {
        info!("is file");
        let content = read_file(&path)?;
        return Ok(content);
    }

    let mut index_path = path.clone();
    index_path.push("index.gmi");

    if index_path.exists() {
        info!("index path {:?}", index_path);
        let content = read_file(&index_path)?;
        let output = format!("20 text/gemini\r\n{}\r\n", content);
        return Ok(output);
    }

    let mut output = String::new();
    output.push_str(&format!("20 text/gemini\r\n"));

    let file_name = path.as_path().file_name().unwrap();

    output.push_str(file_name.to_str().unwrap());
    for entry in path.read_dir().unwrap() {
        let entry = entry.unwrap();
        let entry_path = entry.path();
        let entry_name = entry_path.to_str().unwrap();
        let entry_string = format!("=> gemini://{}/{}", url.host().unwrap(), entry_name);
        output.push_str(&entry_string);
    }
    output.push_str("\r\n");

    Ok(output)
}

fn handle_client(stream: &mut TlsStream<TcpStream>) {
    let request = match read_request(stream) {
        Ok(value) => value,
        Err(_) => panic!(),
    };

    let request = String::from_utf8(request).unwrap();
    info!("request {}", request);

    let url = parse_url(request).unwrap();

    if url.scheme() != "gemini" {
        panic!("invalid scheme")
    }

    let response = build_response(url).unwrap();
    stream.write(response.as_bytes()).unwrap();

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
