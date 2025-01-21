
use std::{
    collections::HashMap, default, env, io::Error as IoError, net::SocketAddr, sync::{Arc, Mutex}
};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use futures_util::{future, pin_mut, stream::TryStreamExt, StreamExt};

use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::Message;
use std::string::*;
use std::thread::spawn;
use tokio_tungstenite::*;
use tokio_tungstenite::tungstenite::{
    accept_hdr,
    handshake::server::{Request, Response},
};

//Unique ID For each conversation
//might have to think of a better method soon
type UUID = i32;

//Contains the addresses of a chat (either pm/group)
pub struct Client {
    pub sender_addr: Option<SocketAddr>,
    pub sender: Option<Tx>,

    pub current_convo: Option<UUID>,
}

#[derive(Debug)]
pub struct Conversation {
    pub name: String,
    pub id: UUID,
    pub clients: HashMap<SocketAddr, Tx>
}

impl Default for Client {
    fn default() -> Self {
        Client {
            sender_addr: None,
            sender: None, 
            current_convo: None,
        }
    }
}

type Tx = UnboundedSender<Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Client>>>;
type ConvoMap = Arc<Mutex<Vec<Conversation>>>;

pub const SERVER_ADDR: &str = "127.0.0.1:1234";

#[tokio::main]
async fn main() -> Result<(), IoError> {
    //our hashmap for a client address and its mpsc sink 
    let convo = ConvoMap::new(Mutex::new(Vec::<Conversation>::new()));
    
    let client_maps = Arc::new(Mutex::new(HashMap::new()));
    
    // Create the event loop and TCP listener for the server
    let try_server = TcpListener::bind(SERVER_ADDR).await;
    let server_listener = try_server.expect("failed to bind");
    println!("Listening on {}", SERVER_ADDR);

    // Separate task for each connection to prevent blocking
    while let Ok((stream, addr)) = server_listener.accept().await {
        tokio::spawn(handle_connection(convo.clone(), client_maps.clone(), stream, addr));
    }

    Ok(())

}

async fn handle_connection(convo_map: ConvoMap, client_maps: PeerMap, raw_stream: TcpStream, addr: SocketAddr) {
    println!("Incoming connection from {}", addr);

    // Establishes a websocket connection with the incoming address
    // gets a websocket
    let ws_stream = accept_async(raw_stream)
        .await
        .expect("Error during Websocket Handshake");
    println!("Websocket Connection Established at {}", addr);

    // Insert the write part of a new channel into the peer map.
    let (tx, rx) = unbounded();
    let mut client = Client::default();
    client.sender = Some(tx);
    client_maps.lock().unwrap().insert(addr, client);

    // split the websocket connection into sink (send)/stream (receiver)
    let (sink, stream) = ws_stream.split();

    //now we have two async operations running concurrenctly

    // 1. if this current websocket has a message coming in
    // we iterate through all other addresses and send the message through their tx
    let broadcast_incoming = stream.try_for_each(|msg| {
        println!("Received a message from {}: {}", addr, msg.to_text().unwrap());
        let peers = client_maps.lock().unwrap();
        let mut convos = convo_map.lock().unwrap();
        let current_client = peers.get(&&addr).unwrap();
        let tx = current_client.sender.as_ref().unwrap();

        let mut receiving_port: bool = false;

        if receiving_port {
            if let Ok(current_addr) = msg.clone().to_text(){
                let mut receiver_addr: Vec<SocketAddr> = vec![];
                for addr in current_addr.split(" ").into_iter() {
                    println!("split addr: {}", addr);
                    if let Ok(new_addr) = addr.parse::<SocketAddr>() {
                        receiver_addr.push(new_addr);
                    }
                }
                if !receiver_addr.is_empty() {
                    //get all the tx
                    let mut to_be_clients = HashMap::<SocketAddr, Tx>::new();
                    for receiver in receiver_addr.iter() {
                        if let Some(received) = peers.get(receiver) {
                            to_be_clients.insert(*receiver, received.sender.as_ref().unwrap().clone());
                        }
                    }

                    let new_convo = Conversation {
                        name: String::new(),
                        id: convos.len().try_into().unwrap(),
                        clients: to_be_clients,
                    };
                    //add
                    convos.push(new_convo);
                }
            }
            receiving_port = false;
        }
        else {
            match msg.clone().to_text().unwrap() {
                "--list" => {
                    tx.unbounded_send(Message::Text(
                        format!("Current Conversations YOU ({}) are in", &addr).into())).unwrap();
                    if let Some(client_conversations) = get_conversations_uuid(addr, &convos) {
                        for id in client_conversations.iter() {
                            for convo in convos.iter() {
                                if &convo.id == id {
                                    tx.unbounded_send(Message::Text(format!("{:?}", convo).into())).unwrap();
                                }
                            }
                        }
                    }
                    tx.unbounded_send(Message::Text("End".into())).unwrap();
                },
                "--connect" => {
                    receiving_port = true;
                    tx.unbounded_send(Message::Text("Please enter the full ip-addresses, split multiple with whitespace.".into())).unwrap();
                },
                _ => {
                    // Broadcast to all clients
                    // let broadcast_receivers = peers.iter()
                    // .filter(|(peer_addr, _)| peer_addr != &&addr)
                    // .map(|(_, ws_sink)| ws_sink);
                    
                    // for client in broadcast_receivers {
                    //     for client_tx in client.iter() {
                    //         client_tx.unbounded_send(msg.clone()).unwrap();
                    //     }
                    // }
                }
            };
        }

        future::ok(())
    });

    // 2. we map the previously empty rx to the websocket sink (sender)
    let receive_from_others = rx.map(Ok).forward(sink);

    pin_mut!(broadcast_incoming, receive_from_others);
    future::select(broadcast_incoming, receive_from_others).await;

    println!("{} disconnected", addr);
    client_maps.lock().unwrap().remove(&addr);

}
//test run by echoing the message sent by the incoming_stream back to the original addr

pub fn get_conversations_uuid(addr: SocketAddr, convo: &Vec<Conversation>) -> Option<Vec<UUID>>{
    let mut convos = vec![];
    for c in convo.iter() {
        if c.clients.get(&addr).is_some() {
            convos.push(c.id);
        }
    }
    if convos.is_empty() {
        return None
    } else {
        return Some(convos)
    }
}


