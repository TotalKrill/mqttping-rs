use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc::{channel, Receiver},
    time,
};

use clap::Parser;

use rumqttc::{AsyncClient as Client, Event, MqttOptions, Packet, QoS, Transport};

mod encryption;

async fn connect_and_wait_until_reception(
    topic: &str,
    options: MqttOptions,
) -> Result<Receiver<Instant>, ()> {
    let (client, mut eventloop) = Client::new(options, 10);
    client
        .subscribe(format!("{topic}/+"), QoS::AtMostOnce)
        .await
        .map_err(|_e| ())?;

    loop {
        let notif = eventloop.poll().await.map_err(|_e| ())?;
        if let Event::Incoming(Packet::SubAck(_)) = notif {
            break;
        }
    }

    let (tx, rx) = channel(100);

    tokio::task::spawn(async move {
        loop {
            let notif = eventloop.poll().await.map_err(|_e| ()).unwrap();
            if let Event::Incoming(Packet::Publish(_publish)) = notif {
                tx.send(Instant::now()).await.ok();
            }
        }
    });

    Ok(rx)
}

async fn connect_sender(options: MqttOptions) -> Client {
    let (client, mut eventloop) = Client::new(options, 10);
    tokio::task::spawn(async move { while let Ok(_notif) = eventloop.poll().await {} });
    client
}

fn mean(data: &VecDeque<Duration>) -> Option<Duration> {
    let sum = data.iter().sum::<Duration>().as_micros() as f64;
    let count = data.len();

    match count {
        positive if positive > 0 => Some(Duration::from_micros((sum / count as f64) as u64)),
        _ => None,
    }
}

// fn std_deviation(data: &[i32]) -> Option<f32> {
//     match (mean(data), data.len()) {
//         (Some(data_mean), count) if count > 0 => {
//             let variance = data
//                 .iter()
//                 .map(|value| {
//                     let diff = data_mean - (*value as f32);

//                     diff * diff
//                 })
//                 .sum::<f32>()
//                 / count as f32;

//             Some(variance.sqrt())
//         }
//         _ => None,
//     }
// }

mod cli;

fn get_mqtt_options(matches: &cli::CliOpt) -> (MqttOptions, MqttOptions) {
    let (transport, host, port) = match &matches.broker {
        cli::Broker::Tcp { host, port } => (Transport::Tcp, host.clone(), *port),
        cli::Broker::Ssl { host, port } => (
            Transport::Tls(encryption::create_tls_configuration(matches.insecure)),
            host.clone(),
            *port,
        ),
        // On WebSockets the port is ignored. See https://github.com/bytebeamio/rumqtt/issues/270
        cli::Broker::WebSocket(url) => (Transport::Ws, url.to_string(), 666),
        cli::Broker::WebSocketSsl(url) => (
            Transport::Wss(encryption::create_tls_configuration(matches.insecure)),
            url.to_string(),
            666,
        ),
    };

    let sub_id = format!("{}-receiver", matches.base_client_id);

    let mut mqttoptions = MqttOptions::new(sub_id, &host, port);
    mqttoptions.set_max_packet_size(usize::MAX, usize::MAX);
    mqttoptions.set_transport(transport.clone());

    if let Some(password) = matches.password.clone() {
        let username = matches.username.clone().unwrap();
        mqttoptions.set_credentials(username, password);
    }
    let mqttopt_sub = mqttoptions;

    let pub_id = format!("{}-sender", matches.base_client_id);
    let mut mqttoptions = MqttOptions::new(pub_id, host, port);
    mqttoptions.set_max_packet_size(usize::MAX, usize::MAX);
    mqttoptions.set_transport(transport.clone());

    if let Some(password) = matches.password.clone() {
        let username = matches.username.clone().unwrap();
        mqttoptions.set_credentials(username, password);
    }

    let mqttopt_pub = mqttoptions;
    (mqttopt_pub, mqttopt_sub)
}

#[tokio::main]
pub async fn main() {
    let matches = cli::CliOpt::parse();

    let topic = "mqtt-ping/ping";

    let (mqttopt_pub, mqttopt_sub) = get_mqtt_options(&matches);

    let mut rx = connect_and_wait_until_reception(topic, mqttopt_sub)
        .await
        .unwrap();

    // connect sender,
    let sender = connect_sender(mqttopt_pub).await;
    let _e = sender
        .publish(
            "mqtt-ping/startup",
            QoS::AtMostOnce,
            false,
            matches.payload.clone(),
        )
        .await;

    let mut latencies: Vec<Duration> = Vec::new();

    for i in 0..matches.pings {
        let now = Instant::now();
        let stopic = format!("{topic}/{i}");
        let _e = sender
            .publish(stopic, QoS::AtMostOnce, false, matches.payload.clone())
            .await;
        let received = rx.recv().await.unwrap();
        let latency = received - now;
        latencies.push(latency);
        let latency = latency.as_micros() as f64 / 1000.0;
        println!("{i}: {latency}");
        time::sleep(Duration::from_millis(matches.wait_between as _)).await;
    }

    let mut latencies: VecDeque<Duration> = latencies.into();

    if latencies.is_empty() {
        println!("Got no round trip values");
        return;
    }

    let first = latencies.pop_front().unwrap().as_micros() as f64 / 1000.0;
    if latencies.is_empty() {
        let max = f64::NAN;
        let min = f64::NAN;
        let mean = f64::NAN;
        println!("Cold Path,\tMax,\tMin,\tMean");
        println!("{first},\t\t{max},\t{min},\t{mean}");
        return;
    }
    let max = latencies.iter().max().unwrap();
    let max = max.as_micros() as f64 / 1000.0;
    let min = latencies.iter().min().unwrap();
    let min = min.as_micros() as f64 / 1000.0;
    let mean = mean(&latencies).unwrap().as_micros() as f64 / 1000.0;
    // let stddev = latencies.iter().stddev();
    println!("Cold Path,\tMax,\tMin,\tMean");
    println!("{first},\t\t{max},\t{min},\t{mean}");
}
