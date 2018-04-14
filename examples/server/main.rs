// Copyright (c) 2018 Ivo Wetzel

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Crates ---------------------------------------------------------------------
extern crate lbs;


// External Dependencies ------------------------------------------------------
use lbs::Server;


// Very Basic Lobby Server Setup ----------------------------------------------
fn main() {
    let server: Server<String, String, String, String, String> = Server::new(10);
    server.listen("0.0.0.0:7680").expect("Failed to start server.");
}

