use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::vec::Vec;

use hyper_rustls::ConfigBuilderExt;
use ipnet::IpNet;
use rand::seq::SliceRandom;
use rand::thread_rng;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio_rustls::{TlsConnector, TlsStream};
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

struct Cursor {
    index: usize,
    size: usize,
}

impl Cursor {
    fn new(size: usize) -> Self {
        Cursor { index: 0, size }
    }

    fn next(&mut self) -> usize {
        let current = self.index;
        self.index = (self.index + 1) % self.size;
        current
    }
}

async fn create_tcp_stream(ip: IpAddr) -> TlsStream<TcpStream> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).unwrap();
    socket.set_reuse_address(true).unwrap();
    socket.set_nonblocking(true).unwrap();
    socket.set_freebind_ipv6(true).unwrap();

    socket.bind(&SockAddr::from(SocketAddr::new(ip, 0u16))).unwrap();

    let tcp_stream = TcpSocket::from_std_stream(socket.into());

    let client_config = ClientConfig::builder().with_webpki_roots().with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(
        client_config));

    let stream = tcp_stream.connect(SocketAddr::new(IpAddr::from_str("2606:4700:3030::1").unwrap(), 443).into()).await.unwrap();
    let stream = connector.connect(ServerName::try_from("discord.com").unwrap(), stream).await.unwrap();

    return TlsStream::Client(stream);
}

fn connect_to_discord(socket: &Socket) {}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let prefixes = vec![
        "2001:0470:7192::/48".parse::<IpNet>().expect("Invalid prefix"),
    ];

    let mut sockets: Vec<_> = Vec::new();

    for prefix in prefixes {
        for subnet_ip in prefix.subnets(56).unwrap() {
            sockets.push(create_tcp_stream(subnet_ip.addr()).await);
        }
    }

    let num_sockets = sockets.len();

    println!("We have {} ips", num_sockets);

    sockets.shuffle(&mut thread_rng());


    for i in 0_u128..2_u128.pow(32){

        let mut socket = &mut sockets[i as usize % num_sockets];

        let mut invite_code = Vec::with_capacity(16);
        encode_number_to_string(&mut invite_code, i);
        let invite_code = unsafe { String::from_utf8_unchecked(invite_code) }.into_boxed_str();
        let request_string = format!("GET /api/v9/invites/{} HTTP/1.1\r\nHost: discord.com\r\nConnection: keep-alive\r\n\r\n", invite_code);

        let is_valid = send_http_request(&mut socket, request_string.as_bytes()).await;
        if is_valid{
            println!("{invite_code}");
        }
    }

}

fn encode_number_to_string(target: &mut Vec<u8>, mut number: u128) {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-";

    let base = CHARSET.len();
    while number > 0 {
        let remainder = (number % base as u128) as usize;
        target.push(CHARSET[remainder]);
        number /= base as u128;
    }
    for _ in base..=target.len() {
        target.push(CHARSET[0]);
    }
}

async fn send_http_request(stream: &mut TlsStream<TcpStream>, request: &[u8]) -> bool {
    if let Err(e) = stream.write_all(request).await {
        eprintln!("Failed to write request: {}", e);
        return false;
    }

    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut res = httparse::Response::new(&mut headers);

    let mut buf = vec![0; 4096];

    let n = stream.read(&mut buf).await.unwrap();
    match res.parse(&buf[..n]) {
        Ok(httparse::Status::Complete(_)) => {
            // println!("Response: {:?}", res);
        }
        Ok(httparse::Status::Partial) => {
            eprintln!("Incomplete response");
        }
        Err(e) => {
            eprintln!("Failed to parse response: {:?}", e);
        }
    }

    res.code == Some(200)
}