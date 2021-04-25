use clap::{self, App, Arg};
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

use serde_derive::Deserialize;

const CONFIG_FILE_NAME: &'static str = "config.toml";

#[macro_use]
extern crate log;
use env_logger;

#[derive(Debug)]
enum RequestError {
    UnexpectedClose,
    UrlParseError,
    IoReadError,
}

#[derive(Deserialize)]
struct Config {
    content_root: Option<PathBuf>,
    port: Option<u16>,
    host: Option<String>,
    certs: Certificates,
    debug: Option<String>,
}

impl Config {
    pub fn default() -> Self {
        Self {
            content_root: Some(PathBuf::from("content-root")),
            port: Some(1965),
            host: Some("0.0.0.0".to_string()),
            certs: Certificates::default(),
            debug: Some("info".to_string()),
        }
    }
}

#[derive(Deserialize)]
struct Certificates {
    identity_pfx: PathBuf,
    pfx_passphrase: String,
}

impl Certificates {
    pub fn default() -> Self {
        Self {
            identity_pfx: PathBuf::from("localhost.pfx"),
            pfx_passphrase: String::new(),
        }
    }
}

fn read_config(config_path: PathBuf) -> Config {
    let contents = read_file(&config_path);

    match contents {
        Ok(value) => toml::from_str(&value).expect("error reading config"),
        Err(_) => Config::default(),
    }
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

fn build_header(response: &ResponseStatus) -> String {
    let status_category = (response.status_code / 10) % 10;
    // if response.status is OK (starts with 2) and response.body is Some(_), add it to output
    if status_category == 2 && response.body.is_some() {
        let header = format!("{} {}\r\n", response.status_code, response.mime_type);
        let body = response.body.clone();
        format!("{}{}\r\n", header, body.unwrap())
    } else if status_category == 4 {
        format!("{} not found\r\n", response.status_code)
    } else {
        format!("{} an unknown error occured\r\n", response.status_code)
    }
}

struct ResponseStatus {
    status_code: u32,
    mime_type: String,
    body: Option<String>,
}

impl ResponseStatus {
    pub fn new(status_code: u32, body: Option<String>) -> Self {
        Self {
            status_code,
            mime_type: String::from("text/gemini"),
            body,
        }
    }
}

fn handle_request(url: Url, mut path: PathBuf) -> ResponseStatus {
    if let Some(segments) = url.path_segments() {
        for segment in segments {
            path.push(segment);
        }
    };

    if !path.exists() {
        return ResponseStatus::new(40, None);
    }

    if path.is_file() {
        let (status, content) = match read_file(&path) {
            Ok(value) => (20, Some(value)),
            Err(_) => (40, None),
        };
        return ResponseStatus::new(status, content);
    }

    let mut index_path = path.clone();
    index_path.push("index.gmi");

    if index_path.exists() {
        info!("index path {:?}", index_path);
        let (status, content) = match read_file(&index_path) {
            Ok(value) => (20, Some(value)),
            Err(_) => (40, None),
        };
        return ResponseStatus::new(status, content);
    }

    let mut output = String::new();
    let file_name = path.as_path().file_name().unwrap();
    output.push_str(&format!("# {}\n", file_name.to_str().unwrap()));

    for entry in path.read_dir().unwrap() {
        let entry = entry.unwrap();
        let entry_path = entry.path();
        let entry_path = entry_path.strip_prefix("content-root").unwrap();
        let entry_name = entry_path.to_str().unwrap();
        info!("entry {}", entry_name);
        let entry_string = format!("=> /{} {}\n", entry_name, entry_name);
        output.push_str(&entry_string);
    }

    ResponseStatus::new(20, Some(output))
}

fn handle_client(stream: &mut TlsStream<TcpStream>, content_root: PathBuf) {
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

    let response = handle_request(url, content_root);

    let output = build_header(&response);

    stream.write(output.as_bytes()).unwrap();

    info!("response {}", response.status_code);
}

fn main() {
    let matches = App::new("GeminiRS")
        .about("a basic gemini server writtern in rust")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Set path to configuration")
                .required(false)
                .takes_value(true),
        )
        .get_matches();

    let config_arg = matches.value_of("config").unwrap_or(CONFIG_FILE_NAME);
    let config_path = PathBuf::from(config_arg);
    let config = read_config(config_path);

    let content_root = config.content_root.unwrap_or(PathBuf::from("content-root"));

    let debug = config.debug.unwrap_or("info".to_string());

    let port = config.port.unwrap_or(1965);

    env_logger::Builder::new().parse_filters(&debug).init();

    let mut file = File::open(config.certs.identity_pfx).unwrap();
    let mut identity = vec![];
    file.read_to_end(&mut identity).unwrap();
    let identity = Identity::from_pkcs12(&identity, &config.certs.pfx_passphrase).unwrap();

    let host = format!("{}:{}", config.host.unwrap(), config.port.unwrap());
    let listener = TcpListener::bind(&host).unwrap();
    let acceptor = TlsAcceptor::new(identity).unwrap();
    let acceptor = Arc::new(acceptor);

    info!("listening on port {}", port);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                info!("new connection");

                let thread_local_content_root = content_root.clone();
                let acceptor = acceptor.clone();
                thread::spawn(move || {
                    let mut stream = acceptor.accept(stream).unwrap();
                    handle_client(&mut stream, thread_local_content_root);
                });
            }
            Err(e) => {
                warn!("{}", e.to_string())
            }
        }
    }
}
