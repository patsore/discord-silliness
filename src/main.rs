#![feature(ip_bits)]

use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::str::FromStr;
use std::time::Duration;

use http_body_util::Empty;
use hyper::body::Bytes;
use hyper::Request;
use hyper_util::client::legacy::connect::Connection;
use hyper_util::rt::TokioIo;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, error, info};
use tracing_subscriber;

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-";
const MAX_LENGTH: usize = 8;

const IP_PREFIX: u128 = 0x20010470719200000000000000000000;
const PREFIX_SIZE: u8 = 48;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    static SEMA: Semaphore = Semaphore::const_new(200);

    info!("Starting the application");

    let mut set = JoinSet::new();

    for i in 0..=u16::MAX {
        if i % 1 == 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        set.spawn(async move {
            let ip_address = IP_PREFIX + ((i as u128) << 64) + 1;
            let ipv6_addr = Ipv6Addr::from_bits(ip_address);
            info!("Sending requests from {:?}", ipv6_addr.to_string());

            let start = i as u32 * 16;
            let _ticket = SEMA.acquire_many(16).await.unwrap();

            let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).unwrap();

            socket.set_only_v6(false).unwrap();
            socket.set_freebind_ipv6(true).unwrap();
            socket.set_keepalive(true).unwrap();
            // socket.set_send_buffer_size(16).unwrap();
            // socket.set_recv_buffer_size(16).unwrap();
            
            let sock_addr: SocketAddr = SocketAddr::new(IpAddr::from(ipv6_addr), 443);
            socket.bind(&sock_addr.into()).unwrap();
            socket.connect(&SocketAddr::new(IpAddr::from_str("2606:4700:3030::1").unwrap(), 443).into()).unwrap();

            let stream: TcpStream = TcpStream::from_std(std::net::TcpStream::from(socket.try_clone().unwrap())).unwrap();
            let io = TokioIo::new(stream);

            let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Failed to perform handshake: {:?}", e);
                    return Vec::new();
                }
            };

            let task_handle = tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    error!("Connection failed: {:?}", err);
                }
            });


            let mut valid_invites = Vec::new();
            for j in start..start + 16 {
                let mut invite_code = Vec::with_capacity(16);
                encode_number_to_string(&mut invite_code, j);
                let invite_code = unsafe { String::from_utf8_unchecked(invite_code) }.into_boxed_str();
                let invite_link = format!("https://discord.com/api/v9/invite/{}", invite_code);

                let req = Request::builder()
                    .method("GET")
                    .uri(invite_link)
                    .header("Host", "discord.com")
                    .header("User-Agent", "curl/8.6.0")
                    .body(Empty::<Bytes>::new()).unwrap();

                info!("Sender ready: {:?}; closed: {:?}", sender.is_ready(), sender.is_closed());

                match sender.send_request(req).await {
                    Ok(response) => {
                        if response.status().is_success() {
                            info!("Found valid invite: {}", &invite_code);
                            valid_invites.push(invite_code);
                        } else {
                            info!("Invalid invite: {}", invite_code);
                        }
                    }
                    Err(e) => {
                        error!("Failed to send request: {:?}", e);
                    }
                }
            }

            valid_invites
        });
    }

    let mut very_valid_invites = Vec::new();

    while let Some(meow) = set.join_next().await {
        match meow {
            Ok(mut vec) => {
                very_valid_invites.append(&mut vec);
            }
            Err(e) => {
                error!("Task failed: {:?}", e);
            }
        }
    }

    info!("Finished. Valid invites: {:?}", very_valid_invites);
}

fn encode_number_to_string(target: &mut Vec<u8>, mut number: u32) {
    let base = CHARSET.len();
    while number > 0 {
        let remainder = (number % base as u32) as usize;
        target.push(CHARSET[remainder]);
        number /= base as u32;
    }
    for _ in base..=target.len() {
        target.push(CHARSET[0]);
    }
}
