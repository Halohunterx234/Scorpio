use std::net::*;



pub struct State {

    //network info
    pub address: Option<SocketAddr>,


}

impl Default for State {
    fn default() -> Self {
        Self {
            address: None
        }
    }
}