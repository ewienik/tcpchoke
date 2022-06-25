use {
    anyhow::Result,
    clap::Parser,
    futures::TryStreamExt,
    std::{ffi::OsString, net::SocketAddr},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::mpsc::{self, Receiver, Sender},
        time::{self, Duration, MissedTickBehavior},
    },
    tokio_stream::wrappers::TcpListenerStream,
};

#[derive(Debug, Parser)]
#[clap(about, version)]
struct Args {
    #[clap(long = "ic", default_value_t = 1)]
    inbound_connection_delay_nanos: u64,
    #[clap(long = "is", default_value_t = 1)]
    inbound_system_delay_nanos: u64,
    #[clap(long = "oc", default_value_t = 1)]
    outbound_connection_delay_nanos: u64,
    #[clap(long = "os", default_value_t = 1)]
    outbound_system_delay_nanos: u64,

    #[clap()]
    listen: SocketAddr,
    #[clap()]
    connect: SocketAddr,
}

fn spawn_connection_delay(
    delay: Duration,
    connection_tx: Sender<Vec<u8>>,
    system_tx: Sender<(Vec<u8>, Sender<Vec<u8>>)>,
) -> Sender<Vec<u8>> {
    let (tx, mut rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel(1);
    let mut interval = time::interval(delay);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            if let Some(value) = rx.recv().await {
                if system_tx
                    .send((value, connection_tx.clone()))
                    .await
                    .is_err()
                {
                    break;
                }
            } else {
                break;
            }
        }
    });
    return tx;
}

fn spawn_system_delay(delay: Duration) -> Sender<(Vec<u8>, Sender<Vec<u8>>)> {
    let (tx, mut rx): (
        Sender<(Vec<u8>, Sender<Vec<u8>>)>,
        Receiver<(Vec<u8>, Sender<Vec<u8>>)>,
    ) = mpsc::channel(1);
    let mut interval = time::interval(delay);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            if let Some((value, next_tx)) = rx.recv().await {
                if next_tx.send(value).await.is_err() {
                    break;
                }
            } else {
                break;
            }
        }
    });
    return tx;
}

const BUFFER_SIZE: usize = 1024;

async fn process_side(
    mut stream: TcpStream,
    mut rx: Receiver<Vec<u8>>,
    tx: Sender<Vec<u8>>,
    delay: Duration,
    system_tx: Sender<(Vec<u8>, Sender<Vec<u8>>)>,
) -> Result<()> {
    let connection_tx = spawn_connection_delay(delay, tx, system_tx);
    loop {
        let mut buffer = vec![0; BUFFER_SIZE];
        tokio::select! {
            size = stream.read(&mut buffer) => {
                let size = match size {
                    Ok(0) | Err(_) => break,
                    Ok(size) => size,
                };
                buffer.resize(size, 0);
                if connection_tx.send(buffer).await.is_err() {
                    break;
                }
            }
            buffer = rx.recv() => {
                if let Some(buffer) = buffer {
                    if stream.write(&buffer).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }
    Ok(())
}

async fn process_connection(
    in_stream: TcpStream,
    addr: SocketAddr,
    inbound_connection_delay: Duration,
    outbound_connection_delay: Duration,
    inbound_system_tx: Sender<(Vec<u8>, Sender<Vec<u8>>)>,
    outbound_system_tx: Sender<(Vec<u8>, Sender<Vec<u8>>)>,
) -> Result<()> {
    let out_stream = TcpStream::connect(addr).await?;
    let (in_tx, in_rx) = mpsc::channel(1);
    let (out_tx, out_rx) = mpsc::channel(1);
    tokio::spawn(process_side(
        in_stream,
        in_rx,
        out_tx,
        inbound_connection_delay,
        inbound_system_tx,
    ));
    tokio::spawn(process_side(
        out_stream,
        out_rx,
        in_tx,
        outbound_connection_delay,
        outbound_system_tx,
    ));
    Ok(())
}

pub async fn run<A, S>(args: A) -> Result<()>
where
    A: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let args = Args::parse_from(args);
    let inbound_system_tx =
        spawn_system_delay(Duration::from_nanos(args.inbound_system_delay_nanos));
    let outbound_system_tx =
        spawn_system_delay(Duration::from_nanos(args.outbound_system_delay_nanos));
    TcpListenerStream::new(TcpListener::bind(args.listen).await?)
        .try_for_each(|stream| {
            let inbound_system_tx = inbound_system_tx.clone();
            let outbound_system_tx = outbound_system_tx.clone();
            async move {
                tokio::spawn(process_connection(
                    stream,
                    args.connect,
                    Duration::from_nanos(args.inbound_connection_delay_nanos),
                    Duration::from_nanos(args.outbound_connection_delay_nanos),
                    inbound_system_tx,
                    outbound_system_tx,
                ));
                Ok(())
            }
        })
        .await?;
    Ok(())
}

pub fn set_panic_hook() {
    use std::{panic, process};

    let default_panic = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        default_panic(info);
        process::exit(1);
    }));
}
