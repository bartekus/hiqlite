use crate::Error;
use axum::response::sse;
use cryptr::utils::b64_encode;
use tokio::task;
use tracing::{debug, error, info, warn};

pub enum NotifyRequest {
    Notify((i64, Vec<u8>)),
    /// Subscribe, and say when the subscription exists.
    ///
    /// F-051: this used to carry the event sender alone, so `api::listen` could only await the
    /// **send** and not the registration. A `send_async` on a bounded channel completes when
    /// the message is in the buffer, which is not when the handler has processed it, so the
    /// server could answer the HTTP request before the subscriber was in the list. The
    /// acknowledgement closes that at the source.
    Listen(
        (
            flume::Sender<Result<sse::Event, Error>>,
            tokio::sync::oneshot::Sender<()>,
        ),
    ),
}

pub fn spawn() -> (
    flume::Sender<NotifyRequest>,
    flume::Receiver<(i64, Vec<u8>)>,
) {
    let (tx_req, rx_req) = flume::unbounded();
    let (tx_local, rx_local) = flume::unbounded();
    task::spawn(handler(rx_req, tx_local));
    (tx_req, rx_local)
}

#[tracing::instrument(level = "debug", skip_all)]
async fn handler(rx_req: flume::Receiver<NotifyRequest>, tx_local: flume::Sender<(i64, Vec<u8>)>) {
    let mut listeners: Vec<flume::Sender<Result<sse::Event, Error>>> = Vec::new();
    let mut remove_indexes = Vec::new();

    while let Ok(req) = rx_req.recv_async().await {
        match req {
            NotifyRequest::Notify((ts, data)) => {
                debug!("new notification from {}", ts);

                if !listeners.is_empty() {
                    let event = sse::Event::default().data(format!("{} {}", ts, b64_encode(&data)));

                    for (idx, listener) in listeners.iter().enumerate() {
                        // unbounded channels can never block
                        if let Err(err) = listener.send(Ok(event.clone())) {
                            error!("Error sending listener Notification: {}", err);
                            remove_indexes.push(idx);
                        }
                    }

                    while let Some(idx) = remove_indexes.pop() {
                        info!("Removing Notification Listener at position {}", idx);
                        listeners.swap_remove(idx);
                    }
                }

                // unbounded channels can never block
                if let Err(err) = tx_local.send((ts, data)) {
                    error!("Error sending local Notification: {}", err);
                    break;
                }
            }
            NotifyRequest::Listen((tx, ack)) => {
                listeners.push(tx);
                info!("New notification listener subscribed");
                // Sent **after** the push, so a caller that awaits it knows the subscriber is
                // in the list and not merely in a channel.
                if ack.send(()).is_err() {
                    warn!("Notification listener went away before its subscription was confirmed");
                }
            }
        }
    }

    debug!("Listen / Notify handler exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::oneshot;

    fn subscribe() -> (
        flume::Sender<Result<sse::Event, Error>>,
        flume::Receiver<Result<sse::Event, Error>>,
        oneshot::Sender<()>,
        oneshot::Receiver<()>,
    ) {
        // `bounded(1)` is what `api::listen` uses, so the test subscriber is the real one.
        let (tx, rx) = flume::bounded(1);
        let (ack, ack_rx) = oneshot::channel();
        (tx, rx, ack, ack_rx)
    }

    /// F-051, at the handler. The acknowledgement is the client's proof that it is in the
    /// listener list, so an event produced after it has been observed cannot be dropped for
    /// want of a listener.
    #[tokio::test]
    async fn an_acknowledged_subscription_receives_an_event_published_immediately_afterwards() {
        let (tx_req, _rx_local) = spawn();
        let (tx, rx, ack, ack_rx) = subscribe();

        tx_req
            .send_async(NotifyRequest::Listen((tx, ack)))
            .await
            .unwrap();

        // Explicit synchronization, not a sleep: this returns exactly when the push happened.
        tokio::time::timeout(Duration::from_secs(5), ack_rx)
            .await
            .expect("the subscription was never acknowledged")
            .expect("the handler dropped the acknowledgement");

        tx_req
            .send_async(NotifyRequest::Notify((42, b"payload".to_vec())))
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv_async())
            .await
            .expect("the event was not delivered to an acknowledged subscriber")
            .unwrap()
            .unwrap();
        let rendered = format!("{event:?}");
        assert!(
            rendered.contains("42"),
            "the delivered event should carry the timestamp, got {rendered}"
        );
    }

    /// The same lesson as `023` B-1: a caller's future can be cancelled, and the acknowledgement
    /// receiver lives in that future. Dropping it must not take the handler down, because the
    /// handler serves every listener on the node.
    #[tokio::test]
    async fn a_caller_that_abandons_its_acknowledgement_never_kills_the_handler() {
        let (tx_req, _rx_local) = spawn();

        let (tx, _rx, ack, ack_rx) = subscribe();
        drop(ack_rx);
        tx_req
            .send_async(NotifyRequest::Listen((tx, ack)))
            .await
            .unwrap();

        // A second, live subscriber proves the handler is still serving after the abandoned one.
        let (tx2, rx2, ack2, ack_rx2) = subscribe();
        tx_req
            .send_async(NotifyRequest::Listen((tx2, ack2)))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), ack_rx2)
            .await
            .expect("the handler stopped serving after an abandoned acknowledgement")
            .unwrap();

        tx_req
            .send_async(NotifyRequest::Notify((7, b"still here".to_vec())))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), rx2.recv_async())
            .await
            .expect("the surviving subscriber received nothing")
            .unwrap()
            .unwrap();
    }

    /// The acknowledgement is per subscription, and it does not consume the event stream: two
    /// acknowledged subscribers both see the same notification.
    #[tokio::test]
    async fn every_acknowledged_subscriber_receives_the_same_event() {
        let (tx_req, _rx_local) = spawn();

        let mut receivers = Vec::new();
        for _ in 0..2 {
            let (tx, rx, ack, ack_rx) = subscribe();
            tx_req
                .send_async(NotifyRequest::Listen((tx, ack)))
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(5), ack_rx)
                .await
                .expect("a subscription was never acknowledged")
                .unwrap();
            receivers.push(rx);
        }

        tx_req
            .send_async(NotifyRequest::Notify((99, b"fanout".to_vec())))
            .await
            .unwrap();

        for (idx, rx) in receivers.iter().enumerate() {
            tokio::time::timeout(Duration::from_secs(5), rx.recv_async())
                .await
                .unwrap_or_else(|_| panic!("subscriber {idx} received nothing"))
                .unwrap()
                .unwrap();
        }
    }
}
