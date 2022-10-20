use clap::{Parser, ValueHint};
use url::Url;

#[derive(Parser)]
pub struct CliOpt {
    /// URL which represents how to connect to the MQTT broker.
    ///
    /// Examples:
    /// `mqtt://localhost`
    /// `mqtt://localhost:1883`
    /// `mqtts://localhost`
    /// `mqtts://localhost:8883`
    /// `ws://localhost/path`
    /// `ws://localhost:9001/path`
    /// `wss://localhost/path`
    /// `wss://localhost:9001/path`
    #[clap(
        short,
        long,
        env = "MQTTPING_BROKER",
        value_hint = ValueHint::Url,
        value_name = "URL",
        default_value = "mqtt://localhost",
        )]
    pub broker: Broker,

    /// Amount of pings to send
    #[arg(long, short, default_value = "10")]
    pub pings: usize,

    /// Amount of tests to run, mainly useful to get better cold path information
    #[arg(long, short, default_value = "10")]
    pub rounds: usize,

    /// Time to wait between pings
    #[arg(long, short, default_value = "10")]
    pub wait_between: usize,

    #[arg(long, default_value = "0123456789")]
    pub payload: String,

    #[arg(long)]
    pub insecure: bool,

    /// Client base to create the sender and receiver clients for the ping transaction
    ///
    /// The client base will be appended with {}-sender, and {}-reciever, for the two differenct clients
    #[arg(
        short = 'i',
        long,
        env = "MQTTPING_CLIENTBASE",
        default_value = "mqttping"
    )]
    pub base_client_id: String,

    /// Username to access the mqtt broker.
    ///
    /// Anonymous access when not supplied.
    #[arg(
        short,
        long,
        env = "MQTTPING_USERNAME",
        value_hint = ValueHint::Username,
        value_name = "STRING",
        requires = "password",
    )]
    pub username: Option<String>,

    /// Password to access the mqtt broker.
    ///
    /// Consider using a connection with TLS to the broker.
    /// Otherwise the password will be transported in plaintext.
    ///
    /// Passing the password via command line is insecure as the password can be read from the history!
    /// You should pass it via environment variable.
    #[arg(
        long,
        env = "MQTTPING_PASSWORD",
        value_hint = ValueHint::Other,
        value_name = "STRING",
        hide_env_values = true,
        requires = "username",
        global = true,
    )]
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Broker {
    Tcp { host: String, port: u16 },
    Ssl { host: String, port: u16 },
    WebSocket(Url),
    WebSocketSsl(Url),
}

impl std::fmt::Display for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Broker::Tcp { host, port } => write!(f, "mqtt://{}:{}", host, port),
            Broker::Ssl { host, port } => write!(f, "mqtts://{}:{}", host, port),
            Broker::WebSocket(url) => write!(f, "{}", url),
            Broker::WebSocketSsl(url) => write!(f, "{}", url),
        }
    }
}

impl std::str::FromStr for Broker {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        anyhow::ensure!(url.has_host(), "Broker requires a Host");

        if matches!(url.scheme(), "mqtt" | "mqtts") {
            anyhow::ensure!(
                url.path().is_empty() || url.path() == "/",
                "TCP connections only use host (and port) but no path"
            );
        }

        if !matches!(url.scheme(), "ws" | "wss") {
            anyhow::ensure!(url.query().is_none(), "URL query is not used");
            anyhow::ensure!(url.username().is_empty(), "Use --username instead");
            anyhow::ensure!(url.password().is_none(), "Use --password instead");
        }

        let broker = match url.scheme() {
            "mqtt" => Self::Tcp {
                host: url.host_str().unwrap().to_string(),
                port: url.port().unwrap_or(1883),
            },
            "mqtts" => Self::Ssl {
                host: url.host_str().unwrap().to_string(),
                port: url.port().unwrap_or(8883),
            },
            "ws" => Self::WebSocket(url),
            "wss" => Self::WebSocketSsl(url),
            _ => anyhow::bail!("Broker URL scheme is not supported"),
        };

        Ok(broker)
    }
}
