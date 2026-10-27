//! Bounded HTTP/1 transport. Stopping closes the listener immediately, permits
//! final response bytes to drain for two seconds, then aborts remaining sockets.
use axum::{Extension, Router, extract::ConnectInfo};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinSet};
use tokio_util::sync::CancellationToken;

pub async fn serve(listener: TcpListener, router: Router, cancel: CancellationToken) {
    let slots = Arc::new(Semaphore::new(32));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            incoming = listener.accept() => {
                let Ok((socket, peer)) = incoming else { break; };
                let Ok(permit) = slots.clone().try_acquire_owned() else { drop(socket); continue; };
                let service = TowerToHyperService::new(router.clone().layer(Extension(ConnectInfo(peer))));
                let stop = cancel.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let mut builder = http1::Builder::new();
                    builder.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(10)).max_buf_size(32 * 1024);
                    let connection = builder.serve_connection(TokioIo::new(socket), service);
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = stop.cancelled() => {
                            connection.as_mut().graceful_shutdown();
                            let _ = tokio::time::timeout(Duration::from_secs(2), &mut connection).await;
                        }
                    }
                });
            }
        }
    }
    drop(listener);
    cancel.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}
