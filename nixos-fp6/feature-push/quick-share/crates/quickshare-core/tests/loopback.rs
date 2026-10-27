use quickshare_core::*;
use std::{path::Path, time::Duration};
use tokio::sync::broadcast;

async fn instance(path: &Path, name: &str) -> std::sync::Arc<Core> {
    Core::start(Config {
        name: name.into(),
        directory: path.into(),
        port: 0,
        interfaces: vec!["lo".into()],
        max_bytes: 1024 * 1024,
    })
    .await
    .unwrap()
}
async fn until(
    rx: &mut broadcast::Receiver<ChannelMessage>,
    predicate: impl Fn(&ChannelMessage) -> bool,
) -> ChannelMessage {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let m = rx.recv().await.unwrap();
            if m.direction == ChannelDirection::LibToFront {
                assert_ne!(
                    m.state,
                    Some(State::Disconnected),
                    "unexpected disconnect: {m:?}"
                );
                if predicate(&m) {
                    return m;
                }
            }
        }
    })
    .await
    .expect("protocol timed out")
}

#[tokio::test]
async fn encrypted_loopback_requires_consent_and_never_overwrites() {
    let tmp = tempfile::tempdir().unwrap();
    let receive = instance(&tmp.path().join("receive"), "Synthetic receiver").await;
    let send = instance(&tmp.path().join("send"), "Synthetic sender").await;
    assert!(!receive.is_visible());
    let mut socket =
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, receive.port()))
            .await
            .unwrap();
    use tokio::io::AsyncReadExt;
    assert_eq!(socket.read(&mut [0u8; 1]).await.unwrap(), 0);
    drop(socket);
    receive.set_visible(true).unwrap();
    let mut incoming = receive.events.subscribe();
    let mut outgoing = send.events.subscribe();
    let source = tmp.path().join("sample.txt");
    let data = vec![0x5au8; 700_000]; // multiple encrypted chunks
    std::fs::write(&source, &data).unwrap();
    std::fs::write(tmp.path().join("receive/sample.txt"), b"original").unwrap();
    let device = Device {
        id: "loopback".into(),
        name: "Synthetic receiver".into(),
        address: format!("127.0.0.1:{}", receive.port()),
    };
    let sid = send
        .send(
            &device,
            OutboundPayload::Files(vec![source.to_str().unwrap().into()]),
            "Synthetic sender".into(),
        )
        .await
        .unwrap();
    let request = until(&mut incoming, |m| {
        m.state == Some(State::WaitingForUserConsent)
    })
    .await;
    assert_eq!(
        std::fs::read_dir(tmp.path().join("receive"))
            .unwrap()
            .count(),
        1
    );
    let sent = until(&mut outgoing, |m| m.state == Some(State::SentIntroduction)).await;
    assert_eq!(
        request.meta.as_ref().unwrap().pin_code,
        sent.meta.as_ref().unwrap().pin_code
    );
    assert_eq!(
        request
            .meta
            .as_ref()
            .unwrap()
            .pin_code
            .as_ref()
            .unwrap()
            .len(),
        4
    );
    receive
        .action(&request.id, ChannelAction::AcceptTransfer)
        .unwrap();
    let finished = until(&mut incoming, |m| m.state == Some(State::Finished)).await;
    assert_eq!(finished.meta.unwrap().ack_bytes, data.len() as u64);
    until(&mut outgoing, |m| {
        m.id == sid && m.state == Some(State::Finished)
    })
    .await;
    assert_eq!(
        std::fs::read(tmp.path().join("receive/sample.txt")).unwrap(),
        b"original"
    );
    assert_eq!(
        std::fs::read(tmp.path().join("receive/sample (1).txt")).unwrap(),
        data
    );
    send.stop().await;
    receive.stop().await;
}

#[tokio::test]
async fn text_url_reject_cancel_and_empty_file() {
    let tmp = tempfile::tempdir().unwrap();
    let receive = instance(&tmp.path().join("receive"), "Synthetic receiver").await;
    let send = instance(&tmp.path().join("send"), "Synthetic sender").await;
    receive.set_visible(true).unwrap();
    let device = Device {
        id: "loopback".into(),
        name: "Synthetic receiver".into(),
        address: format!("127.0.0.1:{}", receive.port()),
    };
    for payload in [
        OutboundPayload::Text("synthetic text".into()),
        OutboundPayload::Url("https://example.invalid/synthetic".into()),
    ] {
        let mut rx = receive.events.subscribe();
        send.send(&device, payload, "Synthetic sender".into())
            .await
            .unwrap();
        let req = until(&mut rx, |m| m.state == Some(State::WaitingForUserConsent)).await;
        receive
            .action(&req.id, ChannelAction::AcceptTransfer)
            .unwrap();
        let done = until(&mut rx, |m| m.state == Some(State::Finished)).await;
        assert!(done.meta.unwrap().text_payload.is_some());
    }
    for action in [ChannelAction::RejectTransfer, ChannelAction::CancelTransfer] {
        let mut rx = receive.events.subscribe();
        send.send(
            &device,
            OutboundPayload::Text("synthetic text".into()),
            "Synthetic sender".into(),
        )
        .await
        .unwrap();
        let req = until(&mut rx, |m| m.state == Some(State::WaitingForUserConsent)).await;
        receive.action(&req.id, action).unwrap();
        until(&mut rx, |m| {
            matches!(m.state, Some(State::Rejected | State::Cancelled))
        })
        .await;
    }
    let empty = tmp.path().join("empty.txt");
    std::fs::write(&empty, b"").unwrap();
    let mut rx = receive.events.subscribe();
    send.send(
        &device,
        OutboundPayload::Files(vec![empty.to_str().unwrap().into()]),
        "Synthetic sender".into(),
    )
    .await
    .unwrap();
    let req = until(&mut rx, |m| m.state == Some(State::WaitingForUserConsent)).await;
    receive
        .action(&req.id, ChannelAction::AcceptTransfer)
        .unwrap();
    until(&mut rx, |m| m.state == Some(State::Finished)).await;
    assert_eq!(
        std::fs::read(tmp.path().join("receive/empty.txt")).unwrap(),
        b""
    );
    assert_eq!(
        std::fs::read_dir(tmp.path().join("receive"))
            .unwrap()
            .count(),
        1
    );
    send.stop().await;
    receive.stop().await;
}
