use crate::cli::CliOpt;
use clap::Parser;
use indicatif::ProgressStyle;
use rumqttc::{AsyncClient as Client, Event, MqttOptions, Packet, QoS, Transport};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc::{channel, Receiver},
    time,
};
mod cli;

mod encryption;

#[tokio::main]
pub async fn main() {
    let matches = crate::CliOpt::parse();

    let topic = "mqtt-ping/ping";

    let mut outputs = Vec::new();

    println!(
        "{}: {} rounds, {} pings",
        matches.broker, matches.rounds, matches.pings,
    );

    println!("{:=>41}", "");

    println!(
        "run:   {:>6} {:>6} {:>6} {:>6} {:>6}",
        "cold", "mean", "med", "max", "min"
    );
    let rounds = matches.rounds;
    for i in 1..=rounds {
        let output = measure_ping(&matches, topic).await;
        let counter = format!("{i:>2}/{rounds}:");
        println!("{counter:<7}{output}");
        outputs.push(output);
    }

    let cold_path: Duration = mean(
        &outputs
            .iter()
            .map(|o| o.cold_path)
            .collect::<VecDeque<Duration>>(),
    )
    .unwrap();

    let mean_value: Duration = mean(
        &outputs
            .iter()
            .map(|o| o.mean)
            .collect::<VecDeque<Duration>>(),
    )
    .unwrap();

    let median: Duration = mean(
        &outputs
            .iter()
            .map(|o| o.median)
            .collect::<VecDeque<Duration>>(),
    )
    .unwrap();
    let max: Duration = mean(
        &outputs
            .iter()
            .map(|o| o.max)
            .collect::<VecDeque<Duration>>(),
    )
    .unwrap();
    let min: Duration = mean(
        &outputs
            .iter()
            .map(|o| o.min)
            .collect::<VecDeque<Duration>>(),
    )
    .unwrap();

    let output = PingOutput {
        cold_path,
        mean: mean_value,
        median,
        max,
        min,
    };

    println!("{:=<41}", "");
    let avgtext = format!("avg:");
    println!("{avgtext:<7}{output}");
}

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
            match eventloop.poll().await {
                Ok(notif) => {
                    if let Event::Incoming(Packet::Publish(_publish)) = notif {
                        tx.send(Instant::now()).await.ok();
                    }
                }
                Err(e) => {
                    log::debug!("Ping reciever exited: {e}");
                    return;
                }
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
fn median(data: &VecDeque<Duration>) -> Option<Duration> {
    let mut data = data.iter().map(|v| *v).collect::<Vec<Duration>>();
    data.sort();
    let mut data: VecDeque<Duration> = data.into();

    while data.len() >= 3 {
        data.pop_back();
        data.pop_front();
    }

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

pub struct PingOutput {
    pub cold_path: Duration,
    pub max: Duration,
    pub min: Duration,
    pub mean: Duration,
    pub median: Duration,
}

impl std::fmt::Display for PingOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:>6} {:>6} {:>6} {:>6} {:>6}",
            self.cold_path.as_millis(),
            self.mean.as_millis(),
            self.median.as_millis(),
            self.max.as_millis(),
            self.min.as_millis(),
        )
    }
}

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
pub async fn measure_ping(matches: &CliOpt, topic: &str) -> PingOutput {
    use indicatif::ProgressBar;

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

    let mut pb = ProgressBar::new(matches.pings as _);
    // pb.set_width(39);
    pb.set_style(
        ProgressStyle::with_template("[{bar:40} {pos:>7}/{len:7}")
            .unwrap()
            .progress_chars("##-"),
    );

    for i in 0..matches.pings {
        pb.inc(1);
        let now = Instant::now();
        let stopic = format!("{topic}/{i}");
        let _e = sender
            .publish(stopic, QoS::AtMostOnce, false, matches.payload.clone())
            .await;
        let received = rx.recv().await.unwrap();
        let latency = received - now;
        latencies.push(latency);
        // let latency = latency.as_micros() as f64 / 1000.0;
        // println!("{i}: {latency}");
        time::sleep(Duration::from_millis(matches.wait_between as _)).await;
    }
    pb.finish_and_clear();

    let mut latencies: VecDeque<Duration> = latencies.into();

    if latencies.is_empty() {
        // println!("Got no round trip values");
        return PingOutput {
            cold_path: Duration::ZERO,
            max: Duration::ZERO,
            min: Duration::ZERO,
            mean: Duration::ZERO,
            median: Duration::ZERO,
        };
    }

    let first = latencies.pop_front().unwrap();
    if latencies.is_empty() {
        let max = Duration::ZERO;
        let min = Duration::ZERO;
        let mean = Duration::ZERO;
        let median = Duration::ZERO;

        return PingOutput {
            cold_path: first,
            max,
            min,
            mean,
            median,
        };
    }
    let max = latencies.iter().max().unwrap().clone();
    let min = latencies.iter().min().unwrap().clone();
    let mean = mean(&latencies).unwrap();
    let median = median(&latencies).unwrap();

    PingOutput {
        cold_path: first,
        max,
        min,
        mean,
        median,
    }
}
