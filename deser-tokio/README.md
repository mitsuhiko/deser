# deser-tokio

Read and write [deser](https://github.com/mitsuhiko/deser) values with
[tokio](https://tokio.rs).  This connects the decoders and encoders of all
deser formats (JSON, CBOR, YAML, TOML, ...) to `AsyncRead` and
`AsyncWrite`, for instance to speak JSON Lines or CBOR sequences over a
socket:

```rust
use deser::{Deserialize, Serialize};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
use deser_tokio::{Reader, Writer};

#[derive(Serialize, Deserialize)]
struct Request {
    id: u64,
    method: String,
}

async fn serve(socket: tokio::net::TcpStream) -> Result<(), deser::Error> {
    let (input, output) = socket.into_split();
    let config = DeserializerConfig::new().trailing(Trailing::Newline);
    let mut requests = Reader::new(input, config.decoder());
    let mut responses = Writer::new(output, SerializerConfig::new().encoder().lines());
    while let Some(request) = requests.read::<Request>().await? {
        responses.write(&request.id).await?;
    }
    Ok(())
}
```

* **Works with every format:** the framing is done by the formats'
  decoders (see `deser::io`), this crate only does the IO.
* **Bounded memory for streams:** only one value is buffered at a time.
  Values are parsed from a complete buffer with the regular (fast)
  parsers, and they can borrow from it (`Reader::read_borrowed`).
* **Multi-threaded runtimes:** the futures are `Send` and can be spawned.
* **Cancellation safe reads:** a read can be used in `tokio::select!`
  without losing data.
* **Errors refer to the stream:** offsets, lines and columns are relative
  to the start of the stream.
* **tokio-util:** with the `codec` feature, `Codec` works with
  `FramedRead`, `FramedWrite` and `Framed`.

For blocking IO use `deser::io` and the formats' `from_reader` and
`to_writer` functions instead.
