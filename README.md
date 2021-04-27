
# Grass
A Gemini server written in Rust

## KEYS
- For development ,self-signed keys, generate with `openssl req -nodes -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -subj '/CN=localhost'`

## Installation
- `git clone https://github.com/connormullett/gemini_rs`
- `cd gemini_rs`
- `cargo install --path .`
