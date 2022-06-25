use std::env;

#[tokio::main]
async fn main() {
    tcpchoke::set_panic_hook();
    tcpchoke::run(env::args_os()).await.unwrap();
}
