use anyhow::{Context, Result};
use hyper::{
  service::{make_service_fn, service_fn},
  upgrade::Upgraded,
  Body, Method, Request, Response, Server,
};

use std::{convert::Infallible, net::SocketAddr};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() {
  let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

  let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handler)) });

  let server = Server::bind(&addr).serve(make_svc);

  if let Err(e) = server.await {
    eprintln!("server error: {}", e);
  }
}

async fn handler(req: Request<Body>) -> Result<Response<Body>> {
  if req.method() == Method::CONNECT {
    let result = connect(req).await;

    if let Err(ref err) = result {
      eprintln!("error handling connect. error={:?}", err);
    };

    result
  } else {
    let result = proxy(req).await;

    if let Err(ref err) = result {
      eprintln!("error proxying. error={:?}", err);
    }

    result
  }
}

async fn connect(req: Request<Body>) -> Result<Response<Body>> {
  let host_addr = req
    .uri()
    .authority()
    .map(|authority| authority.to_string())
    .context("missing request authority")?;

  tokio::spawn(async {
    match hyper::upgrade::on(req).await {
      Err(err) => eprintln!("error upgrading. error={:?}", err),
      Ok(conn) => {
        if let Err(err) = tunnel(conn, host_addr).await {
          eprintln!("error tunneling. error={:?}", err);
        }
      }
    }
  });

  Ok(Response::new(Body::empty()))
}

async fn tunnel(mut conn: Upgraded, host_addr: String) -> Result<()> {
  let mut server = TcpStream::connect(host_addr).await?;

  let (_bytes_copied_from_server, _bytes_copied_from_conn) =
    tokio::io::copy_bidirectional(&mut conn, &mut server).await?;

  Ok(())
}

async fn proxy(req: Request<Body>) -> Result<Response<Body>> {
  let host = req.uri().host().context("uri missing host")?;
  let port = req.uri().port_u16().unwrap_or(80);

  let tcp_stream = TcpStream::connect(format!("{host}:{port}")).await?;

  let (mut sender, conn) = hyper::client::conn::Builder::new()
    .http1_title_case_headers(true)
    .http1_preserve_header_case(true)
    .handshake(tcp_stream)
    .await?;

  tokio::spawn(conn);

  let response = sender.send_request(req).await?;

  Ok(response)
}
