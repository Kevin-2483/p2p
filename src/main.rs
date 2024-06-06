use libp2p::futures::StreamExt;
use libp2p::kad::{record::store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent};
use libp2p::mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p::swarm::{
    NetworkBehaviour, NetworkBehaviourEventProcess, Swarm, SwarmBuilder, SwarmEvent,
};
use libp2p::{development_transport, identity, Multiaddr, NetworkBehaviour, PeerId};
use std::env;
use std::error::Error;
use std::result::Result as StdResult; // 为了避免名称冲突，使用别名
use tokio;
// use libp2p::dns::DnsConfig;
// use libp2p::tcp::GenTcpConfig;
use chrono::Local;
use log::{info, debug, error};
use log4rs::append::console::ConsoleAppender;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use log4rs::init_config;
use log::LevelFilter;


#[derive(NetworkBehaviour)]
#[behaviour(out_event = "MyBehaviourEvent")]
struct MyBehaviour {
    kademlia: Kademlia<MemoryStore>,
    mdns: Mdns,
}

#[derive(Debug)]
enum MyBehaviourEvent {
    Kademlia(KademliaEvent),
    Mdns(MdnsEvent),
}

impl From<KademliaEvent> for MyBehaviourEvent {
    fn from(event: KademliaEvent) -> Self {
        MyBehaviourEvent::Kademlia(event)
    }
}

impl From<MdnsEvent> for MyBehaviourEvent {
    fn from(event: MdnsEvent) -> Self {
        MyBehaviourEvent::Mdns(event)
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for MyBehaviour {
    fn inject_event(&mut self, event: KademliaEvent) {
        // 处理 Kademlia 事件
        match event {
            KademliaEvent::RoutingUpdated { peer, .. } => {
                info!("Kademlia RoutingUpdated: {:?}", peer);
            }
            KademliaEvent::UnroutablePeer { peer } => {
                info!("Kademlia UnroutablePeer: {:?}", peer);
            }
            KademliaEvent::RoutablePeer { peer, .. } => {
                info!("Kademlia RoutablePeer: {:?}", peer);
            }
            KademliaEvent::PendingRoutablePeer { peer, .. } => {
                info!("Kademlia PendingRoutablePeer: {:?}", peer);
            }
            _ => {
                info!("Unhandled Kademlia event: {:?}", event);
            }
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for MyBehaviour {
    fn inject_event(&mut self, event: MdnsEvent) {
        // 处理 mDNS 事件
        match event {
            MdnsEvent::Discovered(peers) => {
                for (peer_id, _) in peers {
                    info!("mDNS discovered: {:?}", peer_id);
                }
            }
            MdnsEvent::Expired(peers) => {
                for (peer_id, _) in peers {
                    info!("mDNS expired: {:?}", peer_id);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> StdResult<(), Box<dyn Error>> {
// 获取当前时间并格式化为文件名
    let now = Local::now();
    let log_file_name = format!("log/output_{}.log", now.format("%Y-%m-%d_%H-%M-%S"));

    // 构建文件 appender
    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}{n}")))
        .build(log_file_name)
        .unwrap();

    // 构建 console appender
    let console_appender = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {l} - {m}{n}")))
        .build();

    // 创建 root 配置
    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(console_appender)))
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .build(
            Root::builder()
                .appender("stdout")
                .appender("file")
                .build(LevelFilter::Info),
        )
        .unwrap();

    // 初始化 log4rs 配置
    init_config(config).unwrap();

    // 创建本地PeerId
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    info!("Local peer id: {:?}", local_peer_id);

    // 创建传输层
    let transport = development_transport(local_key.clone()).await?;

    // 创建Kademlia DHT
    let store = MemoryStore::new(local_peer_id.clone());
    let kademlia_config = KademliaConfig::default();
    let kademlia = Kademlia::with_config(local_peer_id.clone(), store, kademlia_config);

    // 创建mDNS
    let mdns = Mdns::new(MdnsConfig::default()).await?;

    // 创建网络行为
    let behaviour = MyBehaviour { kademlia, mdns };

    // 构建Swarm
    let mut swarm = SwarmBuilder::new(transport, behaviour, local_peer_id)
        .executor(Box::new(|fut| {
            tokio::spawn(fut);
        }))
        .build();

    // 获取环境变量中的 bootstrap_peer_id
    let bootstrap_peer_id_str = match env::var("BOOTSTRAP_PEER_ID") {
        Ok(val) => val,
        Err(_) => {
            error!("Warning: BOOTSTRAP_PEER_ID environment variable is not set, using radmon value");
            PeerId::from_public_key(&identity::PublicKey::Ed25519(identity::ed25519::PublicKey::decode(&[0u8; 32]).unwrap())).to_string()
        }
    };

    // 获取环境变量中的 bootstrap_addr
    let bootstrap_addr_str = match env::var("BOOTSTRAP_ADDR") {
        Ok(val) => val,
        Err(_) => {
            error!("Warning: BOOTSTRAP_ADDR environment variable is not set,using default value:/ip4/127.0.0.1/tcp/8080");
            "/ip4/127.0.0.1/tcp/8080".to_string()
        }
    };
    // 获取环境变量中的 listening_addr_str
    let listening_addr_str = match env::var("LISTENING_ADDR") {
        Ok(val) => val,
        Err(_) => {
            error!("Warning: LISTENING_ADDR environment variable is not set, using default value:/ip4/0.0.0.0/tcp/12345");
            "/ip4/0.0.0.0/tcp/12345".to_string()
        }
    };
    // 添加引导节点
    let bootstrap_peer_id = bootstrap_peer_id_str.parse::<PeerId>()?;
    let bootstrap_addr: Multiaddr = bootstrap_addr_str
        .parse()
        .unwrap();
    swarm
        .behaviour_mut()
        .kademlia
        .add_address(&bootstrap_peer_id, bootstrap_addr);

    // 设置监听地址
    let listen_addr: Multiaddr = env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| listening_addr_str.to_string())
        .parse()
        .unwrap();
    Swarm::listen_on(&mut swarm, listen_addr)?;
    // 连接到其他节点
    // let remote_peer_id = PeerId::from_public_key(&identity::PublicKey::Ed25519(identity::ed25519::PublicKey::decode(&[1u8; 32]).unwrap()));
    // let remote_addr: Multiaddr = env::var("REMOTE_ADDR").unwrap_or_else(|_| "/ip4/127.0.0.1/tcp/8080".to_string()).parse().unwrap();
    // Swarm::dial(&mut swarm, remote_addr.clone()).expect("Failed to dial address");
    // swarm.behaviour_mut().kademlia.add_address(&remote_peer_id, remote_addr);

    // 开始事件循环
    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(MdnsEvent::Discovered(peers))) => {
                for (peer_id, _) in peers {
                    info!("Discovered peer via mDNS: {:?}", peer_id);
                }
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(MdnsEvent::Expired(peers))) => {
                for (peer_id, _) in peers {
                    info!("Expired peer via mDNS: {:?}", peer_id);
                }
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Kademlia(KademliaEvent::RoutingUpdated {
                peer,
                ..
            })) => {
                info!("Discovered peer via Kademlia: {:?}", peer);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("Connected to peer: {:?}", peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                if let Some(err) = cause {
                    info!(
                        "Connection to peer {:?} closed with error: {:?}",
                        peer_id, err
                    );
                } else {
                    info!("Connection to peer {:?} closed.", peer_id);
                }
            }
            _ => {}
        }
    }
}
