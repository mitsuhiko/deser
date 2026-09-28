# deser-tokio

Read and write [deser](https://github.com/mitsuhiko/deser) values with
[tokio](https://tokio.rs).  This connects the stream serializers and
stream deserializers of all deser formats (JSON, CBOR, YAML, TOML, ...) to
`AsyncRead` and `AsyncWrite`, for instance to speak JSON Lines or CBOR
sequences over a socket:

```rust
use deser::{Deserialize, Serialize};
use deser_json::{DeserializerConfig, Serializer, SerializerConfig, StreamDeserializer, Trailing};
use deser_tokio::{Reader, Writer};
use tokio::net::TcpStream;

const READ_LINES: DeserializerConfig =
    DeserializerConfig::new().trailing(Trailing::Newline);
const WRITE_LINES: SerializerConfig =
    SerializerConfig::new().trailing(Trailing::Newline);

#[derive(Serialize, Deserialize)]
struct Request {
    id: u64,
    method: String,
}

async fn serve(socket: TcpStream) -> Result<(), deser::Error> {
    let (input, output) = socket.into_split();
    let mut requests = Reader::new(input, StreamDeserializer::with_config(&READ_LINES));
    let mut responses = Writer::new(output, Serializer::with_config(&WRITE_LINES));
    while let Some(request) = requests.read::<Request>().await? {
        responses.write(&request.id).await?;
    }
    Ok(())
}
```

* **Works with every format:** the stream deserializers of the formats
  split streams into values and their serializers write them without
  doing IO (see `deser::stream`), this crate only does the IO.
* **Bounded memory:** values of formats that support it (JSON and CBOR)
  are deserialized while their input arrives, only incomplete tokens are
  buffered.  Values of other formats are buffered one at a time.  Values
  can also borrow from the buffer (`Reader::read_borrowed`).
* **Multi-threaded runtimes:** the futures are `Send` and can be spawned.
* **Cancellation safe reads:** a read can be used in `tokio::select!`
  without losing data.
* **Errors refer to the stream:** offsets, lines and columns are relative
  to the start of the stream.
* **tokio-util:** with the `codec` feature, `Codec` works with
  `FramedRead`, `FramedWrite` and `Framed`.

For blocking IO use `deser::io` and the formats' `from_reader` and
`to_writer` functions instead.
