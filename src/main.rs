#![feature(ip_bits)]

use std::net::{IpAddr, Ipv6Addr};

use rayon::prelude::*;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-";
const MAX_LENGTH: usize = 8;

const IP_PREFIX: u128 = 0x2001047071920000000000000000;
const PREFIX_SIZE: u8 = 48;


#[tokio::main]
async fn main() {
    static SEMA: Semaphore = Semaphore::const_new(200);

    let mut set = JoinSet::new();

    for i in 0..=u16::MAX {
        set.spawn(async move {
            let ip_address = IP_PREFIX + ((i as u128) << 64) + 1;
            let client = reqwest::ClientBuilder::new()
                .local_address(IpAddr::from(Ipv6Addr::from_bits(ip_address)))
                .user_agent("curl/8.6.0")
                .build()
                .unwrap();

            let start = i as u32 * 16;
            let _ticket = SEMA.acquire_many(16).await.unwrap();

            let mut valid_invites = Vec::new();

            for j in start..start + 16 {
                let mut invite_code = Vec::with_capacity(16);
                encode_number_to_string(&mut invite_code, j);
                let invite_code = unsafe { String::from_utf8_unchecked(invite_code) }.into_boxed_str();
                let invite_link = format!("https://discord.com/api/v9/invite/s{}", invite_code);
                let request = client.get(invite_link).build().unwrap();
                let response = client.execute(request).await.unwrap();
                if response.status().is_success() {
                    valid_invites.push(invite_code);
                }
            }

            valid_invites
        });
    }

    let mut very_valid_invites = Vec::new();

    while let Some(meow) = set.join_next().await {
        let mut vec = meow.unwrap();
        very_valid_invites.append(&mut vec);
    }

    println!("{very_valid_invites:?}")
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