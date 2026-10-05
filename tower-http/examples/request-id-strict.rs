//! In-memory HTTP demonstration of strict request-id middleware.
//!
//! This example runs an HTTP service entirely in memory (using [`hyper`] over a
//! duplex stream) and makes three requests:
//!
//! 1. A legal incoming id is echoed back as-is.
//! 2. A duplicate incoming id is rejected before the service is called (the
//!    handler and generator both run zero times).
//! 3. A missing id is generated and propagated.
//!
//! Run it with:
//!
//! ```sh
//! cargo run -p tower-http --example request-id-strict --features request-id
//! ```
//!
//! [`hyper`]: https://crates.io/crates/hyper

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tower::{Service, ServiceBuilder};
use tower_http::request_id::{MakeRequestId, RequestId};
use tower_http::ServiceBuilderExt;

type RequestBody = Incoming;
type ResponseBody = Full<Bytes>;

/// A [`MakeRequestId`] that counts how many times it is invoked and always
/// produces a legal id.
#[derive(Clone, Default)]
struct CountingGenerator {
    calls: Arc<AtomicUsize>,
}

impl MakeRequestId for CountingGenerator {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // A legal id: non-empty, visible ASCII only.
        Some(RequestId::new(
            format!("gen-{}", self.calls.load(Ordering::SeqCst))
                .parse()
                .unwrap(),
        ))
    }
}

/// The inner handler. It counts how many times it is called and returns an
/// empty response.
async fn handler(
    calls: Arc<AtomicUsize>,
    _req: Request<RequestBody>,
) -> Result<Response<ResponseBody>, std::convert::Infallible> {
    calls.fetch_add(1, Ordering::SeqCst);
    Ok(Response::new(ResponseBody::default()))
}

/// Make one in-memory HTTP request on a fresh connection.
async fn make_request<S>(
    service: &S,
    req: Request<Full<Bytes>>,
) -> Result<Response<Incoming>, hyper::Error>
where
    S: Service<Request<RequestBody>, Response = Response<ResponseBody>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let (client_io, server_io) = tokio::io::duplex(4096);

    let server = hyper::server::conn::http1::Builder::new().serve_connection(
        TokioIo::new(server_io),
        TowerToHyperService::new(service.clone()),
    );

    let (mut sender, conn) =
        hyper::client::conn::http1::handshake(TokioIo::new(client_io)).await?;

    tokio::spawn(async move {
        if let Err(err) = server.await {
            eprintln!("  server error: {err}");
        }
    });
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("  client connection error: {err}");
        }
    });

    sender.send_request(req).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let generator_calls = Arc::new(AtomicUsize::new(0));

    let service = ServiceBuilder::new()
        .set_x_request_id_strict(CountingGenerator {
            calls: generator_calls.clone(),
        })
        .propagate_x_request_id_strict()
        .service_fn({
            let handler_calls = handler_calls.clone();
            move |req: Request<RequestBody>| {
                let handler_calls = handler_calls.clone();
                async move { handler(handler_calls, req).await }
            }
        });

    // 1. Legal incoming id is echoed as-is.
    println!("1. legal incoming id");
    let req = Request::builder()
        .uri("http://localhost/")
        .header("x-request-id", "req-abc-123")
        .body(Full::default())?;
    let res = make_request(&service, req).await?;
    println!("   status: {}", res.status());
    println!(
        "   x-request-id: {:?}",
        res.headers().get("x-request-id")
    );
    assert_eq!(res.headers()["x-request-id"], "req-abc-123");

    // 2. Duplicate incoming id is rejected before the service is called.
    println!("2. duplicate incoming id");
    let req = Request::builder()
        .uri("http://localhost/")
        .header("x-request-id", "dup")
        .header("x-request-id", "dup")
        .body(Full::default())?;
    match make_request(&service, req).await {
        Ok(res) => {
            println!("   unexpected response: {}", res.status());
        }
        Err(err) => {
            println!("   rejected: {err}");
        }
    }

    // 3. Missing id is generated and propagated.
    println!("3. missing id");
    let req = Request::builder()
        .uri("http://localhost/")
        .body(Full::default())?;
    let res = make_request(&service, req).await?;
    println!("   status: {}", res.status());
    println!(
        "   x-request-id: {:?}",
        res.headers().get("x-request-id")
    );
    let generated = res.headers()["x-request-id"].to_str().unwrap().to_owned();
    assert!(generated.starts_with("gen-"));

    println!();
    println!("handler calls:   {}", handler_calls.load(Ordering::SeqCst));
    println!("generator calls: {}", generator_calls.load(Ordering::SeqCst));

    // The duplicate request must not have reached the handler or generator.
    assert_eq!(handler_calls.load(Ordering::SeqCst), 2);
    assert_eq!(generator_calls.load(Ordering::SeqCst), 1);

    Ok(())
}
