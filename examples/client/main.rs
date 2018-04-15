// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Crates ---------------------------------------------------------------------
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate lbs;


// STD Dependencies -----------------------------------------------------------
use std::env;
use std::thread;
use std::time::Duration;


// External Dependencies ------------------------------------------------------
use lbs::{Client, Event, NetworkConfig};


// Configuration --------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config;

impl NetworkConfig for Config {
    type LobbyId = String;
    type LobbyPayload = String;
    type ConnectionIdentifier = String;
    type PreferenceKey = String;
    type PreferenceValue = String;
}

fn client(lobby_id: String, ident: String, action: &str) {

    let mut client: Client<Config> = Client::new("127.0.0.1:7680", ident.clone()).expect("Failed to connect client.");
    loop {
        for event in client.events() {
            match event {
                Event::Connected => {
                    client.identify(ident.clone());
                },
                Event::Ready(ident, addr) => {
                    println!("[Client {}] Ready on UDP {}", ident, addr);
                    if action == "create" {
                        client.lobby_create(lobby_id.clone());

                    } else if action == "join" {
                        client.lobby_join(lobby_id.clone(), None);
                    }
                },
                Event::LobbyJoined(id, connections, pref) => {
                    println!("[Client {}] Lobby joined {} {:?} {:?}", ident, id, connections, pref);
                },
                Event::LobbyJoinRequest(id, conn, payload) => {
                    println!("[Client {}] Lobby join requested by {:?} {:?}", ident, conn, payload);
                    client.lobby_allow_join(conn);
                },
                Event::LobbyPreferenceRequest(id, conn, key, value) => {
                    println!("[Client {}] Lobby pref by {:?} {}={}", ident, conn, key, value);
                    client.lobby_set_preference(key, value);
                    //client.allow_lobby_join(id, addr);
                },
                Event::LobbyUpdated(id, connections, pref) => {
                    println!("[Client {}] Lobby updated {} {:?} {:?}", ident, id, connections, pref);
                },
                Event::LobbyLeft(id) => {
                    println!("[Client {}] Lobby left {}", ident, id);
                },
                Event::LobbyStarted(id, socket, connections, pref) => {
                    println!("[Client {}] Lobby started {} {:?} {:?}", ident, id, connections, pref);
                    // TODO start communication with all other connections
                },
                Event::Error(err) => {
                    println!("[Client {}] Error {:?}", ident, err);
                },
                Event::Disconnected => {
                    println!("[Client {}] Disconnected", ident);
                }
            }

        }
        thread::sleep(Duration::from_millis(30));
    }

}

fn main() {
    let arg: String = env::args().skip(1).next().unwrap_or_else(|| "create".to_string());
    client("Foo".to_string(), "Ivo".to_string(), arg.as_str());
}

