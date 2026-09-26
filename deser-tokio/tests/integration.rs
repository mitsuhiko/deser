use std::time::Duration;

use deser::{Deserialize, ErrorKind, Serialize};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};
use deser_tokio::{Codec, Reader, Writer};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncWriteExt, duplex};
use tokio_util::codec::{FramedRead, FramedWrite};

const LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
const WRITE_LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);

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
        let mut writer = deser::io::Writer::new(Vec::new(), WRITE_LINES);
        for id in 0..10 {
            writer.write(&message(id)).unwrap();
        }
        let out = writer.into_inner();
        for chunk in out.chunks(7) {
            client.write_all(chunk).await.unwrap();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    let mut reader = Reader::new(server, LINES);
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
        let mut writer = Writer::new(client, deser_cbor::SerializerConfig::new());
        for id in 0..100 {
            writer.write(&message(id)).await.unwrap();
        }
    });
    let reader = tokio::spawn(async move {
        let mut reader = Reader::new(server, deser_cbor::DeserializerConfig::new());
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
    let mut reader = Reader::new(server, LINES);
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
    let mut reader = Reader::new(input, LINES);
    assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(1)));
    let err = reader.read::<Message>().await.unwrap_err();
    assert_eq!(err.line(), Some(2));
    assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(2)));
}

#[tokio::test]
async fn test_read_borrowed() {
    let mut reader = Reader::new(&b"\"hello\"\n"[..], LINES);
    let value: &str = reader.read_borrowed().await.unwrap().unwrap();
    assert_eq!(value, "hello");
}

#[tokio::test]
async fn test_stream() {
    let input = &b"1\n2\n3\n"[..];
    let values = Reader::new(input, LINES)
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
    deser_tokio::to_writer(&mut out, deser_json::SerializerConfig::new(), &message(1))
        .await
        .unwrap();
    let value: Message = deser_tokio::from_reader(&out[..], deser_json::DeserializerConfig::new())
        .await
        .unwrap();
    assert_eq!(value, message(1));

    let err =
        deser_tokio::from_reader::<u32, _, _>(&b""[..], deser_json::DeserializerConfig::new())
            .await
            .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = deser_tokio::from_reader::<u32, _, _>(&b"1\n2"[..], LINES)
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
            deser_cbor::DeserializerConfig::new(),
            deser_cbor::SerializerConfig::new(),
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
            deser_cbor::DeserializerConfig::new(),
            deser_cbor::SerializerConfig::new(),
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
        Codec::<_, _, u32>::new(config, deser_json::SerializerConfig::new()),
    )
    .collect::<Vec<_>>()
    .await;
    let values = values.into_iter().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 2, 3]);
}

const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

#[tokio::test]
async fn test_feeding_read_is_cancellation_safe() {
    let (mut client, server) = duplex(64);
    let mut reader = Reader::new(server, STOP);
    // half a value arrives and is deserialized, then the read is cancelled
    client
        .write_all(b"{\"id\": 1, \"text\": \"mes")
        .await
        .unwrap();
    tokio::select! {
        _ = reader.read::<Message>() => panic!("value is incomplete"),
        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
    // a value of another type cannot be read now
    let err = reader.read::<u32>().await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: a value of another type is being read"
    );
    client.write_all(b"sage 1\"}\n").await.unwrap();
    assert_eq!(reader.read::<Message>().await.unwrap(), Some(message(1)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_feeding_across_tasks() {
    let (mut client, server) = duplex(7);
    let writer = tokio::spawn(async move {
        let mut out = Vec::new();
        for id in 0..50 {
            out.extend(deser_json::to_string(&message(id)).unwrap().into_bytes());
            out.push(b' ');
        }
        client.write_all(&out).await.unwrap();
    });
    let reader = tokio::spawn(async move {
        let mut reader = Reader::new(server, STOP).into_stream::<Message>();
        let mut count = 0;
        while let Some(value) = reader.next().await {
            assert_eq!(value.unwrap(), message(count));
            count += 1;
        }
        count
    });
    writer.await.unwrap();
    assert_eq!(reader.await.unwrap(), 50);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_streamed_elements() {
    use deser::Streamed;
    use deser::io::Next;

    #[derive(Debug, PartialEq, Deserialize)]
    struct Feed {
        name: String,
        messages: Streamed<Message>,
    }

    let (mut client, server) = duplex(16);
    let (sent, mut received) = tokio::sync::mpsc::channel::<u64>(1);
    let writer = tokio::spawn(async move {
        client
            .write_all(b"{\"name\": \"feed\", \"messages\": [")
            .await
            .unwrap();
        for id in 0..5 {
            if id > 0 {
                client.write_all(b",").await.unwrap();
            }
            let json = deser_json::to_string(&message(id)).unwrap();
            client.write_all(json.as_bytes()).await.unwrap();
            // the next message is only written once this one arrived
            assert_eq!(received.recv().await, Some(id));
        }
        client.write_all(b"]}").await.unwrap();
    });

    let mut stream =
        Reader::new(server, DeserializerConfig::new()).into_element_stream::<Feed, Message>();
    for id in 0..5 {
        assert_eq!(
            stream.next().await.unwrap().unwrap(),
            Next::Element(message(id))
        );
        sent.send(id).await.unwrap();
    }
    assert_eq!(
        stream.next().await.unwrap().unwrap(),
        Next::Done(Feed {
            name: "feed".into(),
            messages: Streamed::new()
        })
    );
    assert!(stream.next().await.is_none());
    writer.await.unwrap();
}
