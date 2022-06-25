use {
    anyhow::Result,
    clap::Parser,
    futures::{StreamExt, TryStreamExt},
    std::{ffi::OsString, net::SocketAddr},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::mpsc,
    },
    tokio_stream::wrappers::TcpListenerStream,
};

#[derive(Debug, Parser)]
#[clap(about, version)]
struct Args {
    #[clap(long, default_value_t = 1)]
    inbound_packets_per_seconds: usize,
    #[clap(long, default_value_t = 1)]
    inbound_packets_concurrency: usize,
    #[clap(long, default_value_t = 1)]
    outbound_packets_per_seconds: usize,
    #[clap(long, default_value_t = 1)]
    outbound_packets_concurrency: usize,

    #[clap()]
    listen: SocketAddr,
    #[clap()]
    connect: SocketAddr,
}

const BUFFER_SIZE: usize = 1024;

async fn process_side(
    mut stream: TcpStream,
    mut rx: mpsc::Receiver<Vec<u8>>,
    tx: mpsc::Sender<Vec<u8>>,
) -> Result<()> {
    loop {
        let mut buffer = vec![0; BUFFER_SIZE];
        tokio::select! {
            size = stream.read(&mut buffer) => {
                let size = match size {
                    Ok(0) | Err(_) => break,
                    Ok(size) => size,
                };
                buffer.resize(size, 0);
                if tx.send(buffer).await.is_err() {
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

async fn process_connection(in_stream: TcpStream, addr: SocketAddr) -> Result<()> {
    let out_stream = TcpStream::connect(addr).await?;
    let (in_tx, in_rx) = mpsc::channel(10);
    let (out_tx, out_rx) = mpsc::channel(10);
    tokio::spawn(process_side(in_stream, in_rx, out_tx));
    tokio::spawn(process_side(out_stream, out_rx, in_tx));
    Ok(())
}

pub async fn run<A, S>(args: A) -> Result<()>
where
    A: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let args = Args::parse_from(args);
    TcpListenerStream::new(TcpListener::bind(args.listen).await?)
        .try_for_each(|stream| async move {
            tokio::spawn(process_connection(stream, args.connect));
            Ok(())
        })
        .await?;
    Ok(())
}
