//! A JSON Lines server and client with tokio.
//!
//! The server reads requests with a `deser_tokio::Reader` and writes
//! responses with a `deser_tokio::Writer`, every connection is handled on
//! its own task (the futures are `Send`).  The client uses the `Codec` with
//! tokio-util's `Framed`.
use deser::{Deserialize, Serialize};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
use deser_tokio::{Codec, Reader, Writer};
use futures_util::{SinkExt, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::Framed;

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "method", rename_all = "snake_case")]
pub enum Request {
    Add { a: i64, b: i64 },
    Upper { text: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum Response {
    Number(i64),
    Text(String),
    Error(String),
}

const LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);

async fn handle(socket: TcpStream) -> Result<(), deser::Error> {
    let (input, output) = socket.into_split();
    let mut requests = Reader::new(input, LINES.decoder());
    let mut responses = Writer::new(output, SerializerConfig::new().encoder().lines());
    loop {
        let response = match requests.read::<Request>().await {
            Ok(Some(Request::Add { a, b })) => Response::Number(a + b),
            Ok(Some(Request::Upper { text })) => Response::Text(text.to_uppercase()),
            Ok(None) => return Ok(()),
            // a malformed line only fails that request
            Err(err) if err.kind() != deser::ErrorKind::Io => Response::Error(err.to_string()),
            Err(err) => return Err(err),
        };
        responses.write(&response).await?;
    }
}

#[tokio::main]
async fn main() -> Result<(), deser::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                if let Err(err) = handle(socket).await {
                    eprintln!("connection failed: {}", err);
                }
            });
        }
    });

    let codec =
        Codec::<_, _, Response>::new(LINES.decoder(), SerializerConfig::new().encoder().lines());
    let mut client = Framed::new(TcpStream::connect(addr).await?, codec);
    client.send(Request::Add { a: 1, b: 2 }).await?;
    client
        .send(Request::Upper {
            text: "hello".into(),
        })
        .await?;
    client
        .get_mut()
        .write_all(b"{\"method\": \"nope\"}\n")
        .await?;
    for _ in 0..3 {
        let response = client.next().await.unwrap()?;
        println!("{:?}", response);
    }
    Ok(())
}
