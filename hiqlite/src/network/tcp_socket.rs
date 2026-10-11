use crate::Error;
use axum_server::accept::Accept;
use socket2::{Socket, TcpKeepalive};
use std::future::Ready;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
pub use tokio::net::TcpListener;
use tokio::task;
use tracing::debug;

fn standard_keepalive() -> TcpKeepalive {
    TcpKeepalive::new()
        .with_time(Duration::from_secs(60))
        .with_interval(Duration::from_secs(10))
        .with_retries(3)
}

pub async fn create_listening_socket(addr: SocketAddr) -> Result<TcpListener, Error> {
    task::spawn_blocking(move || {
        let socket = if addr.is_ipv4() {
            Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::STREAM,
                Some(socket2::Protocol::TCP),
            )?
        } else {
            Socket::new(
                socket2::Domain::IPV6,
                socket2::Type::STREAM,
                Some(socket2::Protocol::TCP),
            )?
        };
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        socket.set_tcp_nodelay(true)?;

        #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
        {
            socket.set_tcp_cork(false)?;
            socket.set_priority(6)?;
        }

        let keepalive = standard_keepalive();
        socket.set_tcp_keepalive(&keepalive)?;
        socket.set_keepalive(true)?;

        socket.bind(&addr.into())?;
        socket.listen(1024)?;
        Ok(TcpListener::from_std(socket.into())?)
    })
    .await?
}

pub fn configure_tcp_stream(stream: &mut tokio::net::TcpStream) {
    let socket_ref = socket2::SockRef::from(&*stream);

    if let Err(err) = stream.set_nodelay(true) {
        debug!("Failed to set TCP_NODELAY on TCP stream: {err}");
    }
    if let Err(err) = socket_ref.set_tcp_keepalive(&standard_keepalive()) {
        debug!("Failed to configure TCP keepalive on TCP stream: {err}");
    }
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    if let Err(err) = socket_ref.set_priority(6) {
        debug!("Failed to set socket priority on TCP stream: {err}");
    }
}

#[derive(Clone, Copy)]
pub struct ConfiguredStreamAcceptor;

impl<S> Accept<tokio::net::TcpStream, S> for ConfiguredStreamAcceptor {
    type Stream = tokio::net::TcpStream;
    type Service = S;
    type Future = Ready<io::Result<(tokio::net::TcpStream, S)>>;

    fn accept(&self, stream: tokio::net::TcpStream, service: S) -> Self::Future {
        let mut stream = stream;
        configure_tcp_stream(&mut stream);
        std::future::ready(Ok((stream, service)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listeners_and_accepted_streams_enable_keepalives() {
        tokio::time::timeout(Duration::from_secs(2), async {
            let listener = create_listening_socket("127.0.0.1:0".parse().unwrap())
                .await
                .unwrap();
            let socket = socket2::SockRef::from(&listener);
            assert!(socket.keepalive().unwrap());
            assert!(socket.tcp_nodelay().unwrap());
            assert_eq!(
                socket.tcp_keepalive_time().unwrap(),
                Duration::from_secs(60)
            );

            let mut outgoing = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            configure_tcp_stream(&mut outgoing);
            let (incoming, _) = listener.accept().await.unwrap();
            let (incoming, ()) = ConfiguredStreamAcceptor.accept(incoming, ()).await.unwrap();
            for stream in [&outgoing, &incoming] {
                let socket = socket2::SockRef::from(stream);
                assert!(socket.keepalive().unwrap());
                assert!(stream.nodelay().unwrap());
                assert_eq!(
                    socket.tcp_keepalive_time().unwrap(),
                    Duration::from_secs(60)
                );
                assert_eq!(
                    socket.tcp_keepalive_interval().unwrap(),
                    Duration::from_secs(10)
                );
                assert_eq!(socket.tcp_keepalive_retries().unwrap(), 3);
            }
        })
        .await
        .expect("the nonblocking listener must accept a loopback connection");
    }

    #[tokio::test]
    async fn an_occupied_listen_address_returns_an_error() {
        let listener = create_listening_socket("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let result = create_listening_socket(listener.local_addr().unwrap()).await;
        assert!(result.is_err());
    }
}
