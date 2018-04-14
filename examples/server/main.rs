// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Crates ---------------------------------------------------------------------
extern crate lbs;
extern crate log;
extern crate chrono;


// External Dependencies ------------------------------------------------------
use lbs::{NetworkConfig, Server};


// Modules --------------------------------------------------------------------
mod logger;


// Configuration --------------------------------------------------------------
#[derive(Debug, Clone)]
struct Config;

impl NetworkConfig for Config {
    type LobbyId = String;
    type LobbyPayload = String;
    type ConnectionIdentifier = String;
    type PreferenceKey = String;
    type PreferenceValue = String;
}


// Very Basic Lobby Server Setup ----------------------------------------------
fn main() {

    logger::Logger::init().expect("Fatal: Failed to create logger!");

    let server: Server<Config> = Server::new(10);
    server.listen("0.0.0.0:7680").expect("Failed to start server.");

}

