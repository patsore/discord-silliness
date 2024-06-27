#![feature(ip_bits)]

use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::str::FromStr;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::{Buf, Bytes};
use hyper::Request;
use hyper_util::client::legacy::connect::Connection;
use hyper_util::rt::{TokioExecutor, TokioIo};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::net::{TcpSocket, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};
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
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        set.spawn(async move {
            let ip_address = IP_PREFIX + ((i as u128) << 64) + 1;
            let ipv6_addr = Ipv6Addr::from_bits(ip_address);
            // info!("Sending requests from {:?}", ipv6_addr.to_string());

            let start = i as u32 * 16;
            let _ticket = SEMA.acquire_many(16).await.unwrap();

            let mut valid_invites = Vec::new();

            for j in start..start + 16 {
                let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP)).unwrap();

                socket.set_nonblocking(true).unwrap();
                socket.set_freebind_ipv6(true).unwrap();

                let tcp_socket = TcpSocket::from_std_stream(socket.into());
                tcp_socket.set_keepalive(true).unwrap();
                tcp_socket.set_reuseaddr(true).unwrap();
                let sock_addr: SocketAddr = SocketAddr::new(IpAddr::from(ipv6_addr), (j + 1 - start) as u16);
                tcp_socket.bind(sock_addr.into()).unwrap();
                let stream = tcp_socket.connect(SocketAddr::new(IpAddr::from_str("2606:4700:3030::1").unwrap(), 443).into()).await.unwrap();
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

                match sender.send_request(req).await {
                    Ok(response) => {
                        if response.status().is_success() {
                            info!("Found valid invite: {}", &invite_code);
                            valid_invites.push(invite_code);
                        } else {
                            warn!("Invalid invite: {} - {:?} - {:?}", invite_code, &response.status(), String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().into()).unwrap());
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
