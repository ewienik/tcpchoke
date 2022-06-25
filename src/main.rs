use std::env;

pub fn set_panic_hook() {
    use std::{panic, process};

    let default_panic = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_panic(info);
        process::exit(1);
    }));
}

#[tokio::main]
async fn main() {
    set_panic_hook();
    tcpchoke::run(env::args_os()).await.unwrap();
}
