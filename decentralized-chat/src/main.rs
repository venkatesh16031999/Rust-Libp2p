use libp2p::{
    futures::StreamExt, gossipsub, identity, mdns, noise, request_response::{self, ProtocolSupport}, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId, StreamProtocol, SwarmBuilder
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{error::Error, time::Duration};

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    data: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct Response {
    data: Value,
}

#[derive(NetworkBehaviour)]
struct CustomBehaviour {
    request_response: request_response::json::Behaviour<Request, Response>,
    mdns: mdns::tokio::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    peer_id: PeerId,
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut local_chat_messages: Vec<ChatMessage> = Vec::new();

    let local_keypair = identity::Keypair::generate_ed25519();

    let mut swarm = SwarmBuilder::with_existing_identity(local_keypair.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            let request_response_behaviour =
                request_response::json::Behaviour::<Request, Response>::new(
                    [(
                        StreamProtocol::new("/my-json-protocol"),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                );

            let mdns_behaviour =
                mdns::tokio::Behaviour::new(mdns::Config::default(), key.public().to_peer_id())?;

            let gossipsub_behaviour = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub::Config::default(),
            )?;

            Ok(CustomBehaviour {
                request_response: request_response_behaviour,
                mdns: mdns_behaviour,
                gossipsub: gossipsub_behaviour,
            })
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>()?)?;

    println!("Peer {} started", swarm.local_peer_id());

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(CustomBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer, address) in peers {
                    println!("Peer {} discovered", peer);
                    if swarm
                        .behaviour_mut()
                        .request_response
                        .add_address(&peer, address.clone())
                    {
                        println!(
                            "Address {} added to the peer {}",
                            address.clone(),
                            swarm.local_peer_id()
                        );

                        let chat_message = ChatMessage {
                            peer_id: *swarm.local_peer_id(),
                            message: format!("Hello I am {}", swarm.local_peer_id()),
                        };

                        swarm.behaviour_mut().request_response.send_request(
                            &peer,
                            Request {
                                data: json!(&chat_message),
                            },
                        );
                    }
                }
            }
            SwarmEvent::Behaviour(CustomBehaviourEvent::Mdns(mdns::Event::Expired(peers))) => {
                for (peer, address) in peers {
                    println!("Peer {} expired", peer);

                    println!(
                        "Address {} removed from the peer {}",
                        address.clone(),
                        swarm.local_peer_id()
                    );

                    swarm
                        .behaviour_mut()
                        .request_response
                        .remove_address(&peer, &address);
                }
            }
            SwarmEvent::Behaviour(CustomBehaviourEvent::RequestResponse(
                request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                },
            )) => {
                local_chat_messages.push(serde_json::from_str(&request.data.to_string())?);

                println!("{:?}", local_chat_messages);

                let chat_message = ChatMessage {
                    peer_id: *swarm.local_peer_id(),
                    message: format!("Welcome {}!, I am {}", peer, swarm.local_peer_id()),
                };

                swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(
                        channel,
                        Response {
                            data: json!(chat_message),
                        },
                    )
                    .expect("Response failed to send");
            }
            SwarmEvent::Behaviour(CustomBehaviourEvent::RequestResponse(
                request_response::Event::Message {
                    peer,
                    message: request_response::Message::Response { response, .. },
                },
            )) => {
                println!("Response data: {:?}", response.data);
                println!("From: {}", peer);
            }
            _ => {}
        }
    }
}
