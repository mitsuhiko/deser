# tokio-server

```
cargo run -p tokio-server
```

## Why

This shows that deser works in async code. The example is a small
JSON Lines RPC server and client in one binary, both on tokio.
`deser-tokio` provides an async `Reader`/`Writer` (same idea as
`deser::io`) and a tokio-util `Codec` for `Framed`. The futures are
`Send`, so each connection can run on its own spawned task.

## What it shows

- **Server** (`handle`): `deser_tokio::Reader` over the read half of a
  `TcpStream` and `deser_tokio::Writer` over the write half, with the
  JSON stream deserializer and serializer configured for JSON Lines
  (`Trailing::Newline`). Each request line is
  answered with one response line. A malformed request becomes a
  `Response::Error` and only fails that one request. I/O errors end the
  connection (`err.kind() == ErrorKind::Io`).
- **Client**: `Framed::new(stream, Codec::<_, _, Response>::new(de, ser))`
  with the JSON stream deserializer and serializer created from the
  configs, using `SinkExt::send` and `StreamExt::next`.
- Requests use an internally tagged enum (`"method": "add"`). Responses
  use an externally tagged enum.

The server binds to `127.0.0.1:0` (a random free port). The client
connects, sends two valid requests and one invalid raw line, prints the
three responses and exits. No manual interaction is needed.

## What you should see

```
Number(3)
Text("HELLO")
Error("Unexpected: unknown variant `nope` of Request, expected `add` or
  `upper` at line 3 column 18")
```

The error mentions line 3 because it is the third line on that
connection.

## How to read it

`handle` is the server loop, and the second half of `main` is the client.
To try it by hand, change the bind address to a fixed port, remove the
client part, and connect with `nc 127.0.0.1 PORT`. Then type lines such
as `{"method":"add","a":2,"b":3}`.

Related: `streams` (sync `std::io`), `json-lines`.
