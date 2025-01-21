use std::{net::*, time::Duration};

use tokio::time;
use std::thread;
use tokio_tungstenite::tungstenite::{connect, Message};
use Scorpio::connection::server::*;

pub fn connect_to_server() -> () {
    let (mut socket, response) = connect("ws://localhost:1234/socket").expect("Cant connect");

    println!("Connected to the serevr");
    println!("Response HTTP code: {}", response.status());
    println!("Response contains the following headers:");
    for (header, _value) in response.headers() {
        println!("* {header}");
    }

    socket.send(Message::Text("Hello Weboskcet".into())).unwrap();
    
    loop {
        let msg = socket.read().expect("Error reading message");
        println!("Recevied: {msg}");
        thread::sleep(Duration::from_secs(5));
        let send = socket.send("Ping".into()).expect("Error sending message");

    }
}

fn main() {
    connect_to_server();
}