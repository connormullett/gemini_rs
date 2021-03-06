use clap::{self, App, Arg};
use native_tls::{Identity, TlsAcceptor, TlsStream};
use std::sync::Arc;
use std::thread;
use std::{
    fs,
    io::{Read, Write},
};
use std::{fs::File, path::PathBuf};
use std::{
    io,
    net::{TcpListener, TcpStream},
};
use url::Url;

use daemonize::Daemonize;
use serde_derive::Deserialize;

const CONFIG_FILE_NAME: &str = "config.toml";
const CONTENT_ROOT: &str = "content-root";

#[macro_use]
extern crate log;

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
    log_level: Option<String>,
    certs: Certificates,
}

impl Config {
    pub fn default() -> Self {
        Self {
            content_root: Some(PathBuf::from("content-root")),
            port: Some(1965),
            host: Some("0.0.0.0".to_string()),
            certs: Certificates::default(),
            log_level: Some("info".to_string()),
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
    let contents = read_file(config_path);

    match contents {
        Ok(value) => toml::from_str(&value).expect("error reading config"),
        Err(_) => Config::default(),
    }
}

fn create_config_folder(config_path: &PathBuf) -> io::Result<()> {
    fs::create_dir(config_path)?;

    let config_file = PathBuf::from(format!(
        "{}{}",
        config_path.to_str().unwrap(),
        CONFIG_FILE_NAME
    ));

    fs::copy("examples/config.toml.example", config_file)?;

    fs::create_dir(format!("{}{}", config_path.to_str().unwrap(), CONTENT_ROOT))?;

    Ok(())
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

fn read_file(path: PathBuf) -> Result<String, RequestError> {
    let content = fs::read_to_string(path).map_err(|error| {
        error!("{}", error);
        RequestError::IoReadError
    })?;
    Ok(content)
}

fn build_header(response: &ResponseStatus) -> String {
    let status_category = (response.status_code / 10) % 10;
    // if response.status is OK (starts with 2) and response.body is Some(_), add it to output
    if status_category == 1 {
        format!("{} {}\r\n", response.status_code, response.meta)
    } else if status_category == 2 && response.body.is_some() {
        let header = format!("{} {}\r\n", response.status_code, response.meta);
        let body = response.body.clone();
        format!("{}{}\r\n", header, body.unwrap())
    } else if status_category == 5 {
        format!("{} not found\r\n", response.status_code)
    } else {
        format!("{} an unknown error occured\r\n", response.status_code)
    }
}

#[derive(Debug)]
struct ResponseStatus {
    status_code: u32,
    meta: String,
    body: Option<String>,
}

impl ResponseStatus {
    pub fn new(status_code: u32, meta: String, body: Option<String>) -> Self {
        Self {
            status_code,
            meta,
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

    if url.path().contains("form.gmi") {
        let content = fs::read_to_string(&mut path);

        if let Err(error) = content {
            return ResponseStatus::new(51, error.to_string(), None);
        }

        let content = content.unwrap();

        match url.query() {
            Some(params) => {
                let content = content.replace("{INPUT}", params);
                let content: Vec<&str> = content
                    .split('\n')
                    .filter(|&line| !line.starts_with('?'))
                    .collect();

                let content = content.join("\n");

                return ResponseStatus::new(20, "text/gemini".to_string(), Some(content));
            }
            None => {
                let prompt = content.trim();
                let mut status_code = 10;
                let prompt = prompt
                    .split('\n')
                    .find(|&line| line.starts_with('?'))
                    .expect("expected first line starting with ?");

                if prompt.starts_with("??") {
                    status_code = 11;
                } else if prompt.starts_with('?') {
                    status_code = 10;
                }

                let meta = prompt.replace("?", "");

                return ResponseStatus::new(status_code, meta, None);
            }
        };
    }

    if !path.exists() {
        return ResponseStatus::new(51, "not found".to_string(), None);
    }

    if path.is_file() {
        let (status, content) = match read_file(path) {
            Ok(value) => (20, Some(value)),
            Err(_) => (51, None),
        };
        return ResponseStatus::new(status, "text/gemini".to_string(), content);
    }

    let mut index_path = path.clone();
    index_path.push("index.gmi");

    if index_path.exists() {
        let (status, content) = match read_file(index_path) {
            Ok(value) => (20, Some(value)),
            Err(_) => (40, None),
        };
        return ResponseStatus::new(status, "text/gemini".to_string(), content);
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

    ResponseStatus::new(20, "text/gemini".to_string(), Some(output))
}

fn handle_client(stream: &mut TlsStream<TcpStream>, content_root: PathBuf) {
    let request = match read_request(stream) {
        Ok(value) => value,
        Err(_) => panic!(),
    };

    let request = String::from_utf8(request).unwrap();
    info!("request {}", request);

    let response = match parse_url(request) {
        Ok(url) => {
            if url.scheme() != "gemini" {
                ResponseStatus::new(59, "Unsupported Scheme".to_string(), None)
            } else {
                handle_request(url, content_root)
            }
        }
        Err(_) => ResponseStatus::new(59, "Bad Request".to_string(), None),
    };

    let output = build_header(&response);

    stream.write_all(output.as_bytes()).unwrap();

    info!("response {}", response.status_code);
}

fn main() {
    let matches = App::new("Grass")
        .about("a basic gemini server writtern in rust")
        .arg(
            Arg::with_name("path")
                .short("p")
                .long("path")
                .value_name("PATH")
                .help("Set path to grass configuration directory. Defaults to /var/grass/")
                .default_value("/var/grass/")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("daemon")
                .short("d")
                .long("daemon")
                .help("start as a daemon: CURRENTLY WIP")
                .required(false)
                .takes_value(false),
        )
        .get_matches();

    let path_arg = matches.value_of("path").unwrap();
    let config_path = PathBuf::from(path_arg);

    if !config_path.exists() {
        match create_config_folder(&config_path) {
            Ok(_) => {
                println!(
                    "Default directory created at /var/grass.\n
        Edit the example config.toml to suit your needs.\n
        Don't forget to generate your certificates.\n
        The content-root directory in the same folder contains\n
        the source code for your server. A placeholder file has been placed to use"
                );
            }
            Err(error) => {
                println!(
                    "Error creating configuration directory: {}",
                    error.to_string()
                );
            }
        }
        std::process::exit(0);
    }

    let config = read_config(config_path);

    let content_root = config
        .content_root
        .unwrap_or_else(|| PathBuf::from("content-root"));

    let log_level = config.log_level.unwrap_or_else(|| "info".to_string());

    let port = config.port.unwrap_or(1965);

    env_logger::Builder::new().parse_filters(&log_level).init();

    if matches.is_present("daemon") {
        info!("Starting daemon");
        let stdout = File::create("/tmp/daemon.out").unwrap();
        let stderr = File::create("/tmp/daemon.err").unwrap();

        let daemonize = Daemonize::new()
            .pid_file("/tmp/test.pid")
            .working_directory("/tmp") // for default behaviour.
            .user("nobody")
            .group("daemon")
            .umask(0o777)
            .stdout(stdout)
            .stderr(stderr)
            .exit_action(|| println!("Executed before master process exits"))
            .privileged_action(|| "Executed before drop privileges");

        match daemonize.start() {
            Ok(_) => println!("Success, daemonized"),
            Err(e) => eprintln!("Error, {}", e),
        }
    }

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
