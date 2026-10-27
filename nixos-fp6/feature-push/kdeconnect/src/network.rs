use crate::{
    app::{App, Device},
    protocol::{self, Identity, Packet},
    tls,
};
use anyhow::{ensure, Result};
use if_addrs::{IfAddr, Ifv4Addr};
use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpSocket, TcpStream, UdpSocket},
    sync::{mpsc, Semaphore},
};

static SESSION: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, Debug)]
pub struct Wifi {
    pub name: String,
    pub address: Ifv4Addr,
}
pub fn wifi_interfaces(names: &[String]) -> Result<Vec<Wifi>> {
    Ok(if_addrs::get_if_addrs()?
        .into_iter()
        .filter_map(|i| {
            if !names.contains(&i.name)
                || !std::path::Path::new(&format!("/sys/class/net/{}/wireless", i.name)).exists()
            {
                return None;
            }
            match i.addr {
                IfAddr::V4(address)
                    if !address.ip.is_loopback() && !address.ip.is_unspecified() =>
                {
                    Some(Wifi {
                        name: i.name,
                        address,
                    })
                }
                _ => None,
            }
        })
        .collect())
}
pub fn same_network(local: &Ifv4Addr, peer: Ipv4Addr) -> bool {
    (u32::from(local.ip) & u32::from(local.netmask)) == (u32::from(peer) & u32::from(local.netmask))
        && !peer.is_multicast()
        && !peer.is_unspecified()
        && local.broadcast != Some(peer)
}
pub async fn connect_from(local: IpAddr, peer: SocketAddr) -> Result<TcpStream> {
    let socket = TcpSocket::new_v4()?;
    socket.bind(SocketAddr::new(local, 0))?;
    Ok(tokio::time::timeout(Duration::from_secs(5), socket.connect(peer)).await??)
}
pub async fn run(app: Arc<App>) -> Result<()> {
    let udp = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, protocol::PORT)).await?;
    udp.set_broadcast(true)?;
    let limit = Arc::new(Semaphore::new(16));
    let mut listeners = BTreeMap::<Ipv4Addr, tokio::task::JoinHandle<()>>::new();
    let mut attempts = HashMap::<(Ipv4Addr, String), Instant>::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(10));
    let mut last_broadcast = Instant::now() - Duration::from_secs(60);
    let mut buffer = vec![0u8; protocol::MAX_IDENTITY + 1];
    loop {
        tokio::select! {
            _ = ticker.tick() => {},
            _ = app.discover.notified() => { last_broadcast = Instant::now() - Duration::from_secs(60); },
            recv = udp.recv_from(&mut buffer) => {
                let (size, source) = recv?;
                if size > protocol::MAX_IDENTITY { continue; }
                let IpAddr::V4(ip) = source.ip() else {continue};
                let Some(wifi) = wifi_interfaces(&app.interfaces)?.into_iter().find(|w|same_network(&w.address, ip)) else {continue};
                let Ok(packet) = Packet::decode(&buffer[..size]) else {continue};
                let Ok(identity) = Identity::parse(&packet) else {continue};
                if identity.id == app.inner.lock().await.store.id {continue;}
                if app.inner.lock().await.devices.contains_key(&identity.id) {continue;}
                let Some(port) = packet.body["tcpPort"].as_u64().filter(|p|(1716..=1764).contains(p)) else {continue};
                let key = (ip, identity.id.clone());
                if attempts.get(&key).is_some_and(|at|at.elapsed() < Duration::from_secs(5)) {continue;}
                if attempts.len() >= 256 {continue;}
                let Ok(permit) = limit.clone().try_acquire_owned() else {continue};
                attempts.insert(key, Instant::now());
                let app = app.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = async {
                        let tcp = connect_from(wifi.address.ip.into(), SocketAddr::new(ip.into(), port as u16)).await?;
                        establish(app.clone(), tcp, Some(identity)).await
                    }.await;
                    if let Err(e) = result { app.event(format!("Connection ended: {e}")).await; }
                });
                continue;
            }
        }
        attempts.retain(|_, t| t.elapsed() < Duration::from_secs(30));
        let interfaces = wifi_interfaces(&app.interfaces)?;
        let removed: Vec<_> = listeners
            .keys()
            .filter(|ip| !interfaces.iter().any(|w| w.address.ip == **ip))
            .copied()
            .collect();
        for ip in removed {
            if let Some(handle) = listeners.remove(&ip) {
                handle.abort();
            }
            let ids: Vec<_> = app
                .inner
                .lock()
                .await
                .devices
                .iter()
                .filter(|(_, d)| d.local.ip() == IpAddr::V4(ip))
                .map(|(id, d)| (id.clone(), d.session))
                .collect();
            for (id, session) in ids {
                disconnect(&app, &id, session).await;
            }
        }
        for wifi in &interfaces {
            if listeners.contains_key(&wifi.address.ip) {
                continue;
            }
            let listener = match TcpListener::bind((wifi.address.ip, protocol::PORT)).await {
                Ok(l) => l,
                Err(e) => {
                    app.event(format!("Cannot listen on {}: {e}", wifi.name))
                        .await;
                    continue;
                }
            };
            let app2 = app.clone();
            let limit = limit.clone();
            let address = wifi.address.clone();
            let handle = tokio::spawn(async move {
                while let Ok((tcp, source)) = listener.accept().await {
                    let IpAddr::V4(ip) = source.ip() else {
                        continue;
                    };
                    if !same_network(&address, ip) {
                        continue;
                    }
                    let Ok(permit) = limit.clone().try_acquire_owned() else {
                        continue;
                    };
                    let app = app2.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Err(e) = establish(app.clone(), tcp, None).await {
                            app.event(format!("Connection ended: {e}")).await;
                        }
                    });
                }
            });
            listeners.insert(wifi.address.ip, handle);
        }
        if last_broadcast.elapsed() >= Duration::from_secs(30) {
            let packet = {
                let i = app.inner.lock().await;
                Packet::identity(&i.store.id, &i.store.name, 8).encode()?
            };
            for wifi in &interfaces {
                if !listeners.contains_key(&wifi.address.ip) {
                    continue;
                }
                if let Some(broadcast) = wifi.address.broadcast {
                    // Bind the sending socket to the Wi-Fi address so route selection
                    // cannot send announcements through a cellular default route.
                    if let Ok(socket) = UdpSocket::bind((wifi.address.ip, 0)).await {
                        socket.set_broadcast(true)?;
                        let _ = socket.send_to(&packet, (broadcast, protocol::PORT)).await;
                    }
                }
            }
            last_broadcast = Instant::now();
        }
    }
}
/// Exposed for synthetic loopback interoperability tests. Production callers
/// admit connections only through the configured Wi-Fi interface listeners.
pub async fn establish(
    app: Arc<App>,
    mut tcp: TcpStream,
    discovered: Option<Identity>,
) -> Result<()> {
    let peer_addr = tcp.peer_addr()?;
    let local = tcp.local_addr()?;
    tcp.set_nodelay(true)?;
    socket2::SockRef::from(&tcp).set_tcp_keepalive(
        &socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(30))
            .with_interval(Duration::from_secs(10))
            .with_retries(3),
    )?;
    let outgoing = discovered.is_some();
    let identity = if let Some(identity) = discovered {
        let i = app.inner.lock().await;
        let mut hello = Packet::identity(&i.store.id, &i.store.name, identity.version);
        if identity.version >= 8 {
            hello.body["targetDeviceId"] = identity.id.clone().into();
            hello.body["targetProtocolVersion"] = identity.version.into();
        }
        tcp.write_all(&hello.encode()?).await?;
        identity
    } else {
        let mut reader = BufReader::with_capacity(1, tcp);
        let packet = tokio::time::timeout(
            Duration::from_secs(5),
            protocol::read_packet(&mut reader, protocol::MAX_IDENTITY),
        )
        .await??;
        let identity = Identity::parse(&packet)?;
        let i = app.inner.lock().await;
        if let Some(target) = packet.body["targetDeviceId"].as_str() {
            ensure!(target == i.store.id, "wrong target device");
        }
        if let Some(version) = packet.body["targetProtocolVersion"].as_u64() {
            ensure!(version == 8, "wrong target version");
        }
        tcp = reader.into_inner();
        identity
    };
    ensure!(
        identity.id != app.inner.lock().await.store.id,
        "self connection"
    );
    let tls = app.credentials.handshake(tcp, outgoing).await?;
    let cert = tls
        .ssl()
        .peer_certificate()
        .ok_or_else(|| anyhow::anyhow!("missing certificate"))?;
    ensure!(
        tls::common_name(&cert)? == identity.id,
        "certificate CN does not match identity"
    );
    let fingerprint = tls::fingerprint(&cert)?;
    app.inner
        .lock()
        .await
        .store
        .verify(&identity.id, &fingerprint, identity.version)?;
    let (read, mut write) = tokio::io::split(tls);
    let mut read = BufReader::new(read);
    let identity = if identity.version >= 8 {
        let hello = {
            let i = app.inner.lock().await;
            Packet::identity(&i.store.id, &i.store.name, 8).encode()?
        };
        write.write_all(&hello).await?;
        let secure = tokio::time::timeout(
            Duration::from_secs(5),
            protocol::read_packet(&mut read, protocol::MAX_IDENTITY),
        )
        .await??;
        let secure = Identity::parse(&secure)?;
        identity.same_peer(&secure)?;
        secure
    } else {
        identity
    };
    let id = identity.id.clone();
    let session = SESSION.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::channel::<Packet>(64);
    {
        let mut i = app.inner.lock().await;
        i.store.verify(&id, &fingerprint, identity.version)?;
        ensure!(i.devices.len() < 64, "device capacity reached");
        // Keep the existing channel during simultaneous discovery. No session
        // may replace an active pairing dialog or carry over its consent.
        ensure!(!i.devices.contains_key(&id), "device already connected");
        if let Some(p) = i.store.peers.get_mut(&id) {
            if identity.version > p.version {
                p.version = identity.version;
                i.store.save()?;
            }
        }
        i.devices.insert(
            id.clone(),
            Device {
                identity,
                fingerprint,
                cert,
                address: peer_addr,
                local,
                session,
                tx,
                pending: None,
                battery: None,
            },
        );
        i.last_event = "Device connected over TLS.".into();
    }
    let result: Result<()> = tokio::select! {
        result = async {
            loop {
                // Wait for data before starting the frame deadline. The entire
                // read loop is one future, so outgoing writes cannot cancel and
                // discard a partially read frame.
                read.fill_buf().await?;
                let p = tokio::time::timeout(Duration::from_secs(10), protocol::read_packet(&mut read, protocol::MAX_PACKET)).await??;
                {
                    let i = app.inner.lock().await;
                    ensure!(i.devices.get(&id).is_some_and(|d|d.session == session), "stale session");
                }
                if p.kind == "kdeconnect.pair" { app.pair_packet(&id, &p).await?; }
                else if let Some(plugin) = protocol::plugin_for(&p.kind) {
                    if app.allowed(&id, plugin).await {
                        match tokio::time::timeout(Duration::from_secs(5), crate::plugins::handle(app.clone(), &id, &p)).await {
                            Ok(Ok(()))=>{}, Ok(Err(e))=>app.event(format!("{plugin}: {e}")).await, Err(_)=>app.event(format!("{plugin}: operation timed out")).await,
                        }
                    }
                }
            }
            #[allow(unreachable_code)] Ok(())
        } => result,
        result = async {
            while let Some(p) = rx.recv().await {
                // Recheck consent after dequeue: disabling a plugin invalidates
                // packets already waiting to be written.
                let allowed = p.kind == "kdeconnect.pair" || outgoing_plugin(&p.kind).is_some_and(|plugin| {
                    // Checked asynchronously immediately below.
                    !plugin.is_empty()
                });
                if !allowed {continue;}
                if p.kind != "kdeconnect.pair" {
                    if p.kind == "kdeconnect.mousepad.keyboardstate" {
                        if !app.inner.lock().await.store.peers.contains_key(&id) {continue;}
                    } else if let Some(plugin) = outgoing_plugin(&p.kind) { if !app.allowed(&id, plugin).await {continue;} }
                }
                tokio::time::timeout(Duration::from_secs(10), write.write_all(&p.encode()?)).await??;
            }
            Ok(())
        } => result,
    };
    disconnect(&app, &id, session).await;
    result
}
fn outgoing_plugin(kind: &str) -> Option<&'static str> {
    match kind {
        "kdeconnect.mpris" => Some("media"),
        "kdeconnect.mousepad.keyboardstate" => Some("input"),
        _ => protocol::plugin_for(kind),
    }
}
async fn disconnect(app: &App, id: &str, session: u64) {
    let mut i = app.inner.lock().await;
    if !i.devices.get(id).is_some_and(|d| d.session == session) {
        return;
    }
    i.devices.remove(id);
    if let Some(p) = i.store.peers.get_mut(id) {
        p.plugins.insert("input".into(), false);
    }
    let _ = i.store.save();
    drop(i);
    app.input.lock().await.revoke(id);
    let _ = app.clipboard_state().await;
}
