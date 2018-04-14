extern crate lbs;

use std::thread;
use std::time::Duration;

use lbs::Client;

fn client<C: Send + Sync + Fn() + 'static>(lobby_id: String, ident: String, callback: C) {

    thread::spawn(move || {

        let mut action = 0;
        let mut client: Client<String, String, String, String, String> = Client::new("127.0.0.1:7680", ident.clone()).expect("Failed to connect client.");
        loop {
            client.events();
            match action {
                0 => {
                    client.identify(ident.clone());
                },
                20 => {
                    println!("[Client] Create lobby...");
                    client.create_lobby(lobby_id.clone());
                    callback();
                },
                _ => {}
            }
            thread::sleep(Duration::from_millis(30));
            action += 1;
        }
    });

}

fn main() {

    client("Foo".to_string(), "Ivo".to_string(), || {
        client("Foo".to_string(), "Marko".to_string(), || {

        });
    });

}

