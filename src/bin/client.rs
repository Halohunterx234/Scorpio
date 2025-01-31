use std::env;

use futures_util::{future, pin_mut, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use Scorpio::connection::server::*;

#[tokio::main]
async fn main() {
    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin(stdin_tx));

    let (ws_stream, _) = connect_async(format!("ws://{}/", SERVER_ADDR)).await.expect("Failed to connect to server");
    println!("Websocket Connection has been established");

    let (ws_sink, ws_stream) = ws_stream.split();
    
    let stdin_to_ws = stdin_rx.map(Ok).forward(ws_sink);
    let ws_to_stdout = {
        ws_stream.for_each(|message| async {
            let m = message.unwrap().clone();
            let data = m.clone().into_data();
            println!("msg: {:?}", m.to_text());
            // tokio::io::stdout().write_all(&data).await.unwrap();
            // tokio::io::stdout().flush().await.unwrap();
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
        tx.unbounded_send(Message::binary(buf)).unwrap();
    }
}