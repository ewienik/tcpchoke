use {
    futures::{StreamExt, TryStreamExt},
    std::net::SocketAddr,
    tcpchoke,
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    },
    tokio_stream::wrappers::TcpListenerStream,
};

#[tokio::test]
async fn simple() {
    tcpchoke::set_panic_hook();

    let server = tokio::spawn(
        TcpListenerStream::new(
            TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 12345)))
                .await
                .unwrap(),
        )
        .try_for_each(|mut stream| async move {
            tokio::spawn(async move {
                let mut buffer = vec![0; 20];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(size) => {
                            if stream.write(&buffer[..size]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
            Ok(())
        }),
    );
    let proxy = tokio::spawn(async move {
        tcpchoke::run(["tcpchoke", "127.0.0.1:12346", "127.0.0.1:12345"])
            .await
            .unwrap();
    });
    let clients: Vec<_> = (0..1000_usize)
        .map(|num| {
            tokio::spawn(async move {
                let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], 12346)))
                    .await
                    .unwrap();
                let txt = format!("hello from {num}");
                stream.write(txt.as_bytes()).await.unwrap();
                let mut buffer = vec![0; 20];
                assert_eq!(stream.read(&mut buffer).await.unwrap(), txt.len());
                assert_eq!(&buffer[..txt.len()], txt.as_bytes());
            })
        })
        .collect();
    futures::stream::iter(clients.into_iter())
        .for_each(|handle| async move {
            handle.await.unwrap();
        })
        .await;
    proxy.abort();
    let _ = proxy.await;
    server.abort();
    let _ = server.await;
}
