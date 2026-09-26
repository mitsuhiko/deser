use std::time::Duration;

use deser::{Deserialize, ErrorKind, Serialize};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
use deser_tokio::{Codec, Reader, Writer};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncWriteExt, duplex};
use tokio_util::codec::{FramedRead, FramedWrite};

const LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Message {
    id: u64,
    text: String,
}

fn message(id: u64) -> Message {
    Message {
        id,
        text: format!("message {id}"),
    }
}

#[tokio::test]
async fn test_values_in_small_chunks() {
    let (mut client, server) = duplex(3);
    let writer = tokio::spawn(async move {
        let mut out = Vec::new();
        let mut encoder = SerializerConfig::new().encoder().lines();
        for id in 0..10 {
            deser::io::Encoder::encode(&mut encoder, &message(id), &mut out).unwrap();
        }
        for chunk in out.chunks(7) {
            client.write_all(chunk).await.unwrap();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    let mut reader = Reader::new(server, LINES.decoder());
    for id in 0..10 {
        assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(id)));
    }
    assert_eq!(reader.read::<Message>().await.unwrap(), None);
    writer.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_futures_are_send() {
    let (client, server) = duplex(64);
    // spawning requires the futures to be `Send`
    let writer = tokio::spawn(async move {
        let mut writer = Writer::new(client, deser_cbor::Encoder::default());
        for id in 0..100 {
            writer.write(&message(id)).await.unwrap();
        }
    });
    let reader = tokio::spawn(async move {
        let mut reader = Reader::new(server, deser_cbor::Decoder::default());
        let mut count = 0;
        while let Some(value) = reader.read::<Message>().await.unwrap() {
            assert_eq!(value, message(count));
            count += 1;
        }
        count
    });
    writer.await.unwrap();
    assert_eq!(reader.await.unwrap(), 100);
}

#[tokio::test]
async fn test_read_is_cancellation_safe() {
    let (mut client, server) = duplex(64);
    let mut reader = Reader::new(server, LINES.decoder());
    // half a value arrives, then the read is cancelled
    client.write_all(b"{\"id\": 1, ").await.unwrap();
    tokio::select! {
        _ = reader.read::<Message>() => panic!("value is incomplete"),
        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
    client
        .write_all(b"\"text\": \"message 1\"}\n")
        .await
        .unwrap();
    assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(1)));
}

#[tokio::test]
async fn test_errors_continue() {
    let input = &b"{\"id\": 1, \"text\": \"message 1\"}\n{\"id\": \"x\"}\n{\"id\": 2, \"text\": \"message 2\"}\n"[..];
    let mut reader = Reader::new(input, LINES.decoder());
    assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(1)));
    let err = reader.read::<Message>().await.unwrap_err();
    assert_eq!(err.line(), Some(2));
    assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(2)));
}

#[tokio::test]
async fn test_read_borrowed() {
    let mut reader = Reader::new(&b"\"hello\"\n"[..], LINES.decoder());
    let value: &str = reader.read_borrowed().await.unwrap().unwrap();
    assert_eq!(value, "hello");
}

#[tokio::test]
async fn test_stream() {
    let input = &b"1\n2\n3\n"[..];
    let values = Reader::new(input, LINES.decoder())
        .into_stream::<u32>()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        values.into_iter().collect::<Result<Vec<_>, _>>().unwrap(),
        [1, 2, 3]
    );
}

#[tokio::test]
async fn test_from_reader_and_to_writer() {
    let mut out = Vec::new();
    deser_tokio::to_writer(&mut out, deser_json::Encoder::default(), &message(1))
        .await
        .unwrap();
    let value: Message = deser_tokio::from_reader(&out[..], deser_json::Decoder::default())
        .await
        .unwrap();
    assert_eq!(value, message(1));

    let err = deser_tokio::from_reader::<u32, _, _>(&b""[..], deser_json::Decoder::default())
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = deser_tokio::from_reader::<u32, _, _>(&b"1\n2"[..], LINES.decoder())
        .await
        .unwrap_err();
    assert_eq!(err.line(), Some(2));
}

#[tokio::test]
async fn test_codec() {
    let (client, server) = duplex(16);
    let mut sink = FramedWrite::new(
        client,
        Codec::<_, _, Message>::new(
            deser_cbor::Decoder::default(),
            deser_cbor::Encoder::default(),
        ),
    );
    let writer = tokio::spawn(async move {
        for id in 0..5 {
            sink.send(message(id)).await.unwrap();
        }
    });
    let values = FramedRead::new(
        server,
        Codec::<_, _, Message>::new(
            deser_cbor::Decoder::default(),
            deser_cbor::Encoder::default(),
        ),
    )
    .collect::<Vec<_>>()
    .await;
    writer.await.unwrap();
    let values = values.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, (0..5).map(message).collect::<Vec<_>>());
}

#[tokio::test]
async fn test_codec_values_at_the_end() {
    // numbers are only complete at the end of the stream
    let config = DeserializerConfig::new().trailing(Trailing::Stop);
    let values = FramedRead::new(
        &b"1 2 3"[..],
        Codec::<_, _, u32>::new(config.decoder(), deser_json::Encoder::default()),
    )
    .collect::<Vec<_>>()
    .await;
    let values = values.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2, 3]);
}
