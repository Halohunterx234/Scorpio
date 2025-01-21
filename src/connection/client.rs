use std::env;

use futures_util::{future, pin_mut, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::connection::server::*;
use crate::state::state::*;
use crate::ui::ui_builder::*;

#[tokio::main]
async fn main() {
    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin(stdin_tx));

    let (ws_stream, _) = connect_async(SERVER_ADDR).await.expect("Failed to connect to server");
    println!("Websocket Connection has been established");

    //set up ui
    let mut state = State::default();
    let mut ui_state = Ui_State::default();

    let (ws_sink, ws_stream) = ws_stream.split();
    
    let stdin_to_ws = stdin_rx.map(Ok).forward(ws_sink);
    let ws_to_stdout = {
        ws_stream.for_each(|message| async {
            let data = message.unwrap().into_data();
            tokio::io::stdout().write_all(&data).await.unwrap();
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;
}


// Our helper method which will read data from stdin and send it along
// sender provided
async fn read_stdin(tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let mut stdin = tokio::io::stdin();
    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);
        tx.unbounded_send(Message::Text(buf.try_into().unwrap())).unwrap();
    }
}