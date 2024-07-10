#![feature(ip_bits)]

use std::fs::OpenOptions;
use std::io::Write;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1::{Connection, SendRequest};
use hyper::Request;
use hyper_rustls::ConfigBuilderExt;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;
use tracing::{error, info};
use tracing_subscriber;

//Vanity invites are case-insensitive
const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789-";


const BRUTEFORCE_RANGE: u128 = 35_u128.pow(8);
const MAX_REQUESTS_PER_SECOND: usize = 750;
const MAX_CONCURRENT_REQUESTS: usize = 750 * 6;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let ip_prefix: u128 = std::env::var("IP_PREFIX").expect("Couldn't find IP_PREFIX environment variable.").parse::<u128>().expect("Couldn't parse IP_PREFIX as u128.") + 1;
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file>", args[0]);
        std::process::exit(1);
    }

    let file_name = &args[1];
    let mut file = match OpenOptions::new().write(true).create(true).open(file_name) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to open file '{}': {}", file_name, e);
            std::process::exit(1);
        }
    };

    tracing_subscriber::fmt::init();

    info!("Starting the application");

    let (tx, mut rx) = mpsc::channel(100);

    //handles maximum number of concurrent requests
    static SEMA: Semaphore = Semaphore::const_new(MAX_CONCURRENT_REQUESTS);

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        tokio::pin!(interval);

        for i in (0..BRUTEFORCE_RANGE).step_by(MAX_REQUESTS_PER_SECOND) {
            interval.as_mut().tick().await;
            let mut tickets = SEMA.acquire_many(MAX_REQUESTS_PER_SECOND as u32).await.unwrap();
            for j in 0..MAX_REQUESTS_PER_SECOND {
                let t = tickets.split(1).unwrap();
                let tx = tx.clone();
                tokio::spawn(
                    async move {
                        let _t = t;
                        let ip_address = ip_prefix + ((rand::random::<u16>() as u128) << 64);

                        let ipv6_addr = Ipv6Addr::from_bits(ip_address);
                        let (mut sender, conn) = get_client(IpAddr::from(ipv6_addr)).await.unwrap();

                        let handle = tokio::task::spawn(async move {
                            if let Err(err) = conn.await {
                                error!("Connection failed: {:?}", err);
                            }
                        });

                        let mut invite_code = Vec::with_capacity(16);
                        encode_number_to_string(&mut invite_code, i + j as u128);
                        let invite_code = unsafe { String::from_utf8_unchecked(invite_code) }.into_boxed_str();
                        let invite_link = format!("https://discord.com/api/v9/invites/{}?with_counts=true", invite_code);

                        let req = Request::get(invite_link)
                            .header("Host", "discord.com")
                            .header("User-Agent", "curl/8.6.0")
                            .body(Empty::<Bytes>::new()).unwrap();

                        match sender.send_request(req).await {
                            Ok(response) => {
                                if response.status().is_success() {
                                    let contents = response.into_body().collect().await.unwrap().to_bytes();
                                    let v: Value = serde_json::from_slice(&contents).unwrap();
                                    let guild_id = v["guild_id"].as_str().unwrap().to_string();
                                    let channel_id = v["channel"].get("id").unwrap().as_str().unwrap().to_string();
                                    let user_count = v["approximate_member_count"].as_number().unwrap().as_u64().unwrap();
                                    tx.send(Some((guild_id, channel_id, invite_code, user_count))).await.unwrap();
                                }else{
                                    tx.send(None).await.unwrap();
                                }
                            }
                            Err(e) => {
                                error!("Failed to send request: {:?}", e);
                            }
                        }

                        handle.abort();
                    });
            }
        }
    });

    let mut valid_invites: u128 = 0;
    let mut invalid_invites: u128 = 0;

    while let Some(result) = rx.recv().await {
        match result {
            Some(data) => {
                valid_invites += 1;
                file.write_all(format!("{},{},{},{}\n", data.0, data.1, data.2, data.3).as_bytes()).expect("Failed to write to file");
            }
            None => {
                invalid_invites += 1;
            }
        }
        std::io::stdout().write(format!("\r{} - valid invites, {} - invalid invites, {} - requests pending", valid_invites, invalid_invites, MAX_CONCURRENT_REQUESTS - SEMA.available_permits()).as_bytes()).unwrap();
        std::io::stdout().flush().unwrap();
    }

    info!("Done");
}

fn encode_number_to_string(target: &mut Vec<u8>, mut number: u128) {
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

async fn get_client(addr: IpAddr) -> Result<(SendRequest<Empty<Bytes>>, Connection<TokioIo<TlsStream<TcpStream>>, Empty<Bytes>>), ()> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).unwrap();

    socket.set_nonblocking(true).unwrap();
    socket.set_freebind_ipv6(true).unwrap();

    let tcp_socket = TcpSocket::from_std_stream(socket.into());

    tcp_socket.set_keepalive(true).unwrap();
    tcp_socket.set_reuseaddr(true).unwrap();
    let sock_addr: SocketAddr = SocketAddr::new(addr, 0u16);
    tcp_socket.bind(sock_addr.into()).unwrap();

    let client_config = ClientConfig::builder().with_webpki_roots().with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(
        client_config));

    let stream = tcp_socket.connect(SocketAddr::new(IpAddr::from_str("2606:4700:3030::1").unwrap(), 443).into()).await.unwrap();
    let stream = connector.connect(ServerName::try_from("discord.com").unwrap(), stream).await.unwrap();

    let io = TokioIo::new(stream);
    match hyper::client::conn::http1::handshake(io).await {
        Ok(conn) => Ok(conn),
        Err(e) => {
            error!("Failed to perform handshake: {:?}", e);
            return Err(());
        }
    }
}