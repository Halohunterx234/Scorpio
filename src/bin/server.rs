
use std::{
    collections::HashMap, default, env, io::Error as IoError, net::SocketAddr, str::FromStr, sync::{Arc, Mutex}
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
#[derive(Debug, Clone)]
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

pub enum IO_STATE {
    SEND,
    CREATE_NEW_CONVO,
    CONNECT_TO_CONVO
}

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
    let mut receiving_port: IO_STATE = IO_STATE::SEND;

    let broadcast_incoming = stream.try_for_each(|msg| {
        let mut peers = client_maps.lock().unwrap();
        let mut convos = convo_map.lock().unwrap();
        let current_client = peers.get_mut(&addr).unwrap();
        let c = current_client.clone();
        let tx = current_client.sender.clone().unwrap();
        
        //commands (--text) comes with non-text data that we need to clean
        let msg_2 = msg.clone();
        let mut binding = msg_2.to_text().unwrap();
        let is_command = binding.chars();
        let chars: Vec<char> = is_command.collect();
        if chars[0] == '-' && chars[1] == '-' {
            let cleaned: &str = binding.trim();
            binding = cleaned;
            println!("Received a message from {}: {}", addr, cleaned);
        }

        match receiving_port {
            IO_STATE::CREATE_NEW_CONVO => {
                if let Ok(current_addr) = msg.clone().to_text(){
                    let mut receiver_addr: Vec<SocketAddr> = vec![];
                    for addr in current_addr.split(" ").into_iter() {
                        println!("split addr: {}", addr);
                        let addr = addr.trim();
                        if let Ok(new_addr) = SocketAddr::from_str(addr) {
                            println!("adding new ip: {}", new_addr);
                            receiver_addr.push(new_addr);
                        }
                    }
                    if !receiver_addr.is_empty() {
                        //add original sender inside
                        receiver_addr.push(addr);
                        
                        println!("connecting new receivers");
                        //get all the tx
                        let mut to_be_clients = HashMap::<SocketAddr, Tx>::new();
                        for receiver in receiver_addr.iter() {
                            if let Some(received) = peers.get(receiver) {
                                // println!("inserting ip {} into convo", receiver);
                                if let Some(receiver_tx) = &received.sender {
                                    receiver_tx.unbounded_send(Message::Text(format!("You have been added to a [Conversation]!").into())).unwrap();
                                }
                                to_be_clients.insert(*receiver, received.sender.as_ref().unwrap().clone());
                            }
                        }
    
                        let new_convo = Conversation {
                            name: String::new(),
                            id: convos.len().try_into().unwrap(),
                            clients: to_be_clients,
                        };
                        //add
                        // println!("adding new convo: {:?}", new_convo);
                        tx.unbounded_send(Message::Text(format!("Created a Conversation with the following people: {:?}", new_convo.clients.clone().into_keys().collect::<Vec<SocketAddr>>()).into())).unwrap();
                        convos.push(new_convo);
                    }
                }
    
                receiving_port = IO_STATE::SEND;
            },
            IO_STATE::SEND => {
                match binding {
                    //---SERVER-SIDED COMMANDS----
                    "--list" => {
                        println!("listing");
                        let intro = Message::Text(format!("Current Conversations YOU ({}) are in:", &addr).into());
                        tx.unbounded_send(intro).unwrap();
                        if let Some(client_conversations) = get_conversations_uuid(addr, &convos) {
                            for id in client_conversations.iter() {
                                for convo in convos.iter() {
                                    if &convo.id == id {
                                        let convo_msg = Message::Text(format!("{:?}", convo).into());
                                        tx.unbounded_send(convo_msg).unwrap();
                                    }
                                }
                            }
                        }
                        tx.unbounded_send(Message::Text("End".into())).unwrap();
                    },
                    "--connect" => {
                        println!("connecting");
                        receiving_port = IO_STATE::CREATE_NEW_CONVO;
                        let connect_prompt = Message::Text("Please enter the full ip-addresses, split multiple with whitespace.".into());
                        tx.unbounded_send(connect_prompt).unwrap();
                        // tx.
                    },
                    "--switch" => {
                        println!("switching");
                        receiving_port = IO_STATE::CONNECT_TO_CONVO;
                        let switch_prompt = Message::Text("Please enter UUID of the conversation you would like to join!".into());
                        tx.unbounded_send(switch_prompt).unwrap();
                    },
                    "--current" => {
                        println!("currenting!");
                        let current = Message::Text(format!("{:?}", c).into());
                        tx.unbounded_send(current).unwrap();
                    }
                    // CONVERSATION/ECHO MESSAGES
                    _ => {
                        if current_client.current_convo.is_some() {
                            for c in convos.iter() {
                                if c.id == current_client.current_convo.unwrap() {
                                    for (ppl_addr, ppl_tx) in c.clients.iter() {
                                        if ppl_addr != &addr {
                                            ppl_tx.unbounded_send(Message::Text(format!("[Convo {}] from {}: {}", c.id, addr.to_string(), msg.clone().to_text().unwrap()).into())).unwrap();
                                        }
                                    }
                                }
                            }
                        } else {
                            let echo = Message::Text(format!("echo: {} (you're not in a conversation)", &msg.to_text().unwrap()).into());
                            tx.unbounded_send(echo).unwrap();
                        }
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
            },
            IO_STATE::CONNECT_TO_CONVO => {
                if let Ok(msg_txt) = msg.to_text() {
                    if let Ok(convo_id) = msg_txt.trim().parse::<i32>() {
                        //check if conversation already exists
                        for c in convos.iter() {
                            if c.id == convo_id {
                                //check that the sender is inside the clients
                                if c.clients.contains_key(&addr) {
                                    println!("swapped {} to convo {}!", &addr, convo_id);
                                    current_client.current_convo = Some(convo_id);
                                }
                            }
                        }
                    }
                }
                receiving_port = IO_STATE::SEND;
            }
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


