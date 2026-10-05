//! Strict request-id handling over an in-memory HTTP service.
//!
//! No socket is opened: requests are driven straight through the tower stack. Run it with:
//!
//! ```not_rust
//! cargo run -p request-id-strict-in-memory
//! ```
//!
//! Three requests are sent:
//! 1. a valid incoming id is echoed back unchanged, even though the handler mutates the header;
//! 2. no id, so the generator is consulted exactly once;
//! 3. a duplicated id header is rejected with `400 Bad Request`.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, Request, Response};
use http_body_util::Full;
use tower::{ServiceBuilder, ServiceExt};
use tower_http::request_id::{
    MakeRequestId, RequestId, StrictPropagateRequestIdLayer, StrictSetRequestIdLayer,
};

type Body = Full<Bytes>;

/// Generates ids `gen-0`, `gen-1`, ... and counts how often it was called.
#[derive(Clone, Default)]
struct CountingGenerator {
    counter: Arc<AtomicUsize>,
}

impl MakeRequestId for CountingGenerator {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        let value = HeaderValue::from_str(&format!("gen-{n}")).unwrap();
        Some(RequestId::new(value))
    }
}

/// The application handler. It rewrites the request header on purpose: strict propagation must
/// still use the id that was sealed before the request reached it.
async fn handler(mut request: Request<Body>) -> Result<Response<Body>, std::convert::Infallible> {
    request.headers_mut().insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("tampered-by-handler"),
    );
    Ok(Response::new(Full::default()))
}

/// Drive a future to completion without an async runtime (the stack does no real I/O).
fn block_on<F: Future>(future: F) -> F::Output {
    struct NoopWaker;
    impl Wake for NoopWaker {
        fn wake(self: Arc<Self>) {}
    }

    let waker = Arc::new(NoopWaker).into();
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

fn request_with_ids(ids: &[&'static str]) -> Request<Body> {
    let mut builder = Request::builder();
    for id in ids {
        builder = builder.header("x-request-id", *id);
    }
    builder.body(Full::default()).unwrap()
}

fn main() {
    let generator = CountingGenerator::default();
    let generator_calls = generator.counter.clone();

    let svc = ServiceBuilder::new()
        .layer(StrictSetRequestIdLayer::x_request_id(generator))
        .layer(StrictPropagateRequestIdLayer::x_request_id())
        .service_fn(handler);

    // 1. valid incoming id: echoed verbatim (the handler's tampering is ignored)
    let response = block_on(svc.clone().oneshot(request_with_ids(&["alpha-123"]))).unwrap();
    println!(
        "valid id     -> status: {:?}, x-request-id: {:?}",
        response.status(),
        response.headers().get("x-request-id")
    );

    // 2. missing id: the generator runs and seals the generated value
    let response = block_on(svc.clone().oneshot(request_with_ids(&[]))).unwrap();
    println!(
        "missing id   -> status: {:?}, x-request-id: {:?}",
        response.status(),
        response.headers().get("x-request-id")
    );

    // 3. duplicated id: rejected before the handler runs, value never echoed
    let response = block_on(svc.clone().oneshot(request_with_ids(&["one", "two"]))).unwrap();
    println!(
        "duplicated   -> status: {:?}, x-request-id: {:?}",
        response.status(),
        response.headers().get("x-request-id")
    );

    println!(
        "generator calls: {} (expected 1; the valid and duplicated requests never generate)",
        generator_calls.load(Ordering::SeqCst)
    );
}
