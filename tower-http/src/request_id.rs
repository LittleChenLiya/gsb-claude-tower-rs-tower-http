//! Set and propagate request ids.
//!
//! # Example
//!
//! ```
//! use http::{Request, Response, header::HeaderName};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::request_id::{
//!     SetRequestIdLayer, PropagateRequestIdLayer, MakeRequestId, RequestId,
//! };
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let handler = tower::service_fn(|request: Request<Full<Bytes>>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(request.into_body()))
//! # });
//! #
//! // A `MakeRequestId` that increments an atomic counter
//! #[derive(Clone, Default)]
//! struct MyMakeRequestId {
//!     counter: Arc<AtomicUsize>,
//! }
//!
//! impl MakeRequestId for MyMakeRequestId {
//!     fn make_request_id<B>(&mut self, request: &Request<B>) -> Option<RequestId> {
//!         let request_id = self.counter
//!             .fetch_add(1, Ordering::SeqCst)
//!             .to_string()
//!             .parse()
//!             .unwrap();
//!
//!         Some(RequestId::new(request_id))
//!     }
//! }
//!
//! let x_request_id = HeaderName::from_static("x-request-id");
//!
//! let mut svc = ServiceBuilder::new()
//!     // set `x-request-id` header on all requests
//!     .layer(SetRequestIdLayer::new(
//!         x_request_id.clone(),
//!         MyMakeRequestId::default(),
//!     ))
//!     // propagate `x-request-id` headers from request to response
//!     .layer(PropagateRequestIdLayer::new(x_request_id))
//!     .service(handler);
//!
//! let request = Request::new(Full::default());
//! let response = svc.ready().await?.call(request).await?;
//!
//! assert_eq!(response.headers()["x-request-id"], "0");
//! #
//! # Ok(())
//! # }
//! ```
//!
//! Additional convenience methods are available on [`ServiceBuilderExt`]:
//!
//! ```
//! use tower_http::ServiceBuilderExt;
//! # use http::{Request, Response, header::HeaderName};
//! # use tower::{Service, ServiceExt, ServiceBuilder};
//! # use tower_http::request_id::{
//! #     SetRequestIdLayer, PropagateRequestIdLayer, MakeRequestId, RequestId,
//! # };
//! # use bytes::Bytes;
//! # use http_body_util::Full;
//! # use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let handler = tower::service_fn(|request: Request<Full<Bytes>>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(request.into_body()))
//! # });
//! # #[derive(Clone, Default)]
//! # struct MyMakeRequestId {
//! #     counter: Arc<AtomicUsize>,
//! # }
//! # impl MakeRequestId for MyMakeRequestId {
//! #     fn make_request_id<B>(&mut self, request: &Request<B>) -> Option<RequestId> {
//! #         let request_id = self.counter
//! #             .fetch_add(1, Ordering::SeqCst)
//! #             .to_string()
//! #             .parse()
//! #             .unwrap();
//! #         Some(RequestId::new(request_id))
//! #     }
//! # }
//!
//! let mut svc = ServiceBuilder::new()
//!     .set_x_request_id(MyMakeRequestId::default())
//!     .propagate_x_request_id()
//!     .service(handler);
//!
//! let request = Request::new(Full::default());
//! let response = svc.ready().await?.call(request).await?;
//!
//! assert_eq!(response.headers()["x-request-id"], "0");
//! #
//! # Ok(())
//! # }
//! ```
//!
//! See [`SetRequestId`] and [`PropagateRequestId`] for more details.
//!
//! # Using `Trace`
//!
//! To have request ids show up correctly in logs produced by [`Trace`] you must apply the layers
//! in this order:
//!
//! ```
//! use tower_http::{
//!     ServiceBuilderExt,
//!     trace::{TraceLayer, DefaultMakeSpan, DefaultOnResponse},
//! };
//! # use http::{Request, Response, header::HeaderName};
//! # use tower::{Service, ServiceExt, ServiceBuilder};
//! # use tower_http::request_id::{
//! #     SetRequestIdLayer, PropagateRequestIdLayer, MakeRequestId, RequestId,
//! # };
//! # use http_body_util::Full;
//! # use bytes::Bytes;
//! # use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let handler = tower::service_fn(|request: Request<Full<Bytes>>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(request.into_body()))
//! # });
//! # #[derive(Clone, Default)]
//! # struct MyMakeRequestId {
//! #     counter: Arc<AtomicUsize>,
//! # }
//! # impl MakeRequestId for MyMakeRequestId {
//! #     fn make_request_id<B>(&mut self, request: &Request<B>) -> Option<RequestId> {
//! #         let request_id = self.counter
//! #             .fetch_add(1, Ordering::SeqCst)
//! #             .to_string()
//! #             .parse()
//! #             .unwrap();
//! #         Some(RequestId::new(request_id))
//! #     }
//! # }
//!
//! let svc = ServiceBuilder::new()
//!     // make sure to set request ids before the request reaches `TraceLayer`
//!     .set_x_request_id(MyMakeRequestId::default())
//!     // log requests and responses
//!     .layer(
//!         TraceLayer::new_for_http()
//!             .make_span_with(DefaultMakeSpan::new().include_headers(true))
//!             .on_response(DefaultOnResponse::new().include_headers(true))
//!     )
//!     // propagate the header to the response before the response reaches `TraceLayer`
//!     .propagate_x_request_id()
//!     .service(handler);
//! #
//! # Ok(())
//! # }
//! ```
//!
//! # Doesn't override existing headers
//!
//! [`SetRequestId`] and [`PropagateRequestId`] wont override request ids if its already present on
//! requests or responses. Among other things, this allows other middleware to conditionally set
//! request ids and use the middleware in this module as a fallback.
//!
//! # Strict mode
//!
//! [`SetRequestIdLayer`] and [`PropagateRequestIdLayer`] are deliberately permissive: they accept
//! whatever header the client sends and never override a header that an inner service puts on a
//! response. [`StrictSetRequestIdLayer`] and [`StrictPropagateRequestIdLayer`] provide an explicit
//! strict mode instead:
//!
//! - A duplicated incoming header (more than one value for the configured header name) is rejected
//!   with `400 Bad Request` before the inner service is called.
//! - A single incoming value must be accepted by the configured [`ValidateRequestId`]
//!   (the default is [`AsciiRequestId`], which accepts non-empty visible ASCII values up to a
//!   configurable maximum length). Otherwise the request is rejected with `400 Bad Request`,
//!   and the offending value is never logged or reflected back.
//! - Once a valid id is known, whether it came from the request or from the
//!   [`MakeRequestId`] generator, it is sealed in the request's extensions. The response is always
//!   populated from that sealed id, never by re-reading the (possibly mutated) request header at
//!   the end of processing.
//! - The generator is only consulted when the request carries no id, and its output must also be
//!   valid. An invalid generated id rejects the request with `500 Internal Server Error`.
//! - If the inner service answers with a different response id, a
//!   [`ResponseRequestIdPolicy`] selects between overwriting it with the sealed id (the default)
//!   and rejecting the response with `500 Internal Server Error`.
//! - Nesting a strict layer inside another strict layer does not generate a second id: the sealed
//!   extension from the outer layer is reused.
//!
//! Strict responses with an error status use an empty body and therefore require the response body
//! type to implement `Default`.
//!
//! ```
//! use http::{Request, Response, StatusCode};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::request_id::{
//!     StrictSetRequestIdLayer, StrictPropagateRequestIdLayer, MakeRequestUuid,
//! };
//! use http_body_util::Full;
//! use bytes::Bytes;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let handler = tower::service_fn(|request: Request<Full<Bytes>>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(request.into_body()))
//! # });
//! #
//! let mut svc = ServiceBuilder::new()
//!     .layer(StrictSetRequestIdLayer::x_request_id(MakeRequestUuid))
//!     .layer(StrictPropagateRequestIdLayer::x_request_id())
//!     .service(handler);
//!
//! // a valid incoming id is echoed back unchanged
//! let request = Request::builder()
//!     .header("x-request-id", "alpha-123")
//!     .body(Full::default())?;
//! let response = svc.ready().await?.call(request).await?;
//! assert_eq!(response.headers()["x-request-id"], "alpha-123");
//!
//! // an illegal id is rejected and never echoed
//! let request = Request::builder()
//!     .header("x-request-id", "bad id")
//!     .body(Full::default())?;
//! let response = svc.ready().await?.call(request).await?;
//! assert_eq!(response.status(), StatusCode::BAD_REQUEST);
//! assert!(response.headers().get("x-request-id").is_none());
//! #
//! # Ok(())
//! # }
//! ```
//!
//! [`ServiceBuilderExt`]: crate::ServiceBuilderExt
//! [`Uuid`]: https://crates.io/crates/uuid
//! [`Trace`]: crate::trace::Trace

use http::{
    header::{HeaderName, HeaderValue},
    Request, Response, StatusCode,
};
use pin_project_lite::pin_project;
use std::task::{ready, Context, Poll};
use std::{future::Future, pin::Pin};
use tower_layer::Layer;
use tower_service::Service;
use uuid::Uuid;

pub(crate) const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Trait for producing [`RequestId`]s.
///
/// Used by [`SetRequestId`].
pub trait MakeRequestId {
    /// Try and produce a [`RequestId`] from the request.
    fn make_request_id<B>(&mut self, request: &Request<B>) -> Option<RequestId>;
}

/// An identifier for a request.
#[derive(Debug, Clone)]
pub struct RequestId(HeaderValue);

impl RequestId {
    /// Create a new `RequestId` from a [`HeaderValue`].
    pub const fn new(header_value: HeaderValue) -> Self {
        Self(header_value)
    }

    /// Gets a reference to the underlying [`HeaderValue`].
    pub const fn header_value(&self) -> &HeaderValue {
        &self.0
    }

    /// Consumes `self`, returning the underlying [`HeaderValue`].
    pub fn into_header_value(self) -> HeaderValue {
        self.0
    }
}

impl From<HeaderValue> for RequestId {
    fn from(value: HeaderValue) -> Self {
        Self::new(value)
    }
}

/// Set request id headers and extensions on requests.
///
/// This layer applies the [`SetRequestId`] middleware.
///
/// See the [module docs](self) and [`SetRequestId`] for more details.
#[derive(Debug, Clone)]
pub struct SetRequestIdLayer<M> {
    header_name: HeaderName,
    make_request_id: M,
}

impl<M> SetRequestIdLayer<M> {
    /// Create a new `SetRequestIdLayer`.
    pub const fn new(header_name: HeaderName, make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        SetRequestIdLayer {
            header_name,
            make_request_id,
        }
    }

    /// Create a new `SetRequestIdLayer` that uses `x-request-id` as the header name.
    pub const fn x_request_id(make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        SetRequestIdLayer::new(X_REQUEST_ID, make_request_id)
    }
}

impl<S, M> Layer<S> for SetRequestIdLayer<M>
where
    M: Clone + MakeRequestId,
{
    type Service = SetRequestId<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        SetRequestId::new(
            inner,
            self.header_name.clone(),
            self.make_request_id.clone(),
        )
    }
}

/// Set request id headers and extensions on requests.
///
/// See the [module docs](self) for an example.
///
/// If [`MakeRequestId::make_request_id`] returns `Some(_)` and the request doesn't already have a
/// header with the same name, then the header will be inserted.
///
/// Additionally [`RequestId`] will be inserted into [`Request::extensions`] so other
/// services can access it.
#[derive(Debug, Clone)]
pub struct SetRequestId<S, M> {
    inner: S,
    header_name: HeaderName,
    make_request_id: M,
}

impl<S, M> SetRequestId<S, M> {
    /// Create a new `SetRequestId`.
    pub const fn new(inner: S, header_name: HeaderName, make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        Self {
            inner,
            header_name,
            make_request_id,
        }
    }

    /// Create a new `SetRequestId` that uses `x-request-id` as the header name.
    pub const fn x_request_id(inner: S, make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        Self::new(inner, X_REQUEST_ID, make_request_id)
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `SetRequestId` middleware.
    pub const fn layer(header_name: HeaderName, make_request_id: M) -> SetRequestIdLayer<M>
    where
        M: MakeRequestId,
    {
        SetRequestIdLayer::new(header_name, make_request_id)
    }
}

impl<S, M, ReqBody, ResBody> Service<Request<ReqBody>> for SetRequestId<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeRequestId,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        if let Some(request_id) = req.headers().get(&self.header_name) {
            if req.extensions().get::<RequestId>().is_none() {
                let request_id = request_id.clone();
                req.extensions_mut().insert(RequestId::new(request_id));
            }
        } else if let Some(request_id) = self.make_request_id.make_request_id(&req) {
            req.extensions_mut().insert(request_id.clone());
            req.headers_mut()
                .insert(self.header_name.clone(), request_id.0);
        }

        self.inner.call(req)
    }
}

/// Propagate request ids from requests to responses.
///
/// This layer applies the [`PropagateRequestId`] middleware.
///
/// See the [module docs](self) and [`PropagateRequestId`] for more details.
#[derive(Debug, Clone)]
pub struct PropagateRequestIdLayer {
    header_name: HeaderName,
}

impl PropagateRequestIdLayer {
    /// Create a new `PropagateRequestIdLayer`.
    pub const fn new(header_name: HeaderName) -> Self {
        PropagateRequestIdLayer { header_name }
    }

    /// Create a new `PropagateRequestIdLayer` that uses `x-request-id` as the header name.
    pub const fn x_request_id() -> Self {
        Self::new(X_REQUEST_ID)
    }
}

impl<S> Layer<S> for PropagateRequestIdLayer {
    type Service = PropagateRequestId<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PropagateRequestId::new(inner, self.header_name.clone())
    }
}

/// Propagate request ids from requests to responses.
///
/// See the [module docs](self) for an example.
///
/// If the request contains a matching header that header will be applied to responses. If a
/// [`RequestId`] extension is also present it will be propagated as well.
#[derive(Debug, Clone)]
pub struct PropagateRequestId<S> {
    inner: S,
    header_name: HeaderName,
}

impl<S> PropagateRequestId<S> {
    /// Create a new `PropagateRequestId`.
    pub const fn new(inner: S, header_name: HeaderName) -> Self {
        Self { inner, header_name }
    }

    /// Create a new `PropagateRequestId` that uses `x-request-id` as the header name.
    pub const fn x_request_id(inner: S) -> Self {
        Self::new(inner, X_REQUEST_ID)
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `PropagateRequestId` middleware.
    pub const fn layer(header_name: HeaderName) -> PropagateRequestIdLayer {
        PropagateRequestIdLayer::new(header_name)
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for PropagateRequestId<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = PropagateRequestIdResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let request_id = req
            .headers()
            .get(&self.header_name)
            .cloned()
            .map(RequestId::new);

        PropagateRequestIdResponseFuture {
            inner: self.inner.call(req),
            header_name: self.header_name.clone(),
            request_id,
        }
    }
}

pin_project! {
    /// Response future for [`PropagateRequestId`].
    pub struct PropagateRequestIdResponseFuture<F> {
        #[pin]
        inner: F,
        header_name: HeaderName,
        request_id: Option<RequestId>,
    }
}

impl<F, B, E> Future for PropagateRequestIdResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut response = ready!(this.inner.poll(cx))?;

        if let Some(current_id) = response.headers().get(&*this.header_name) {
            if response.extensions().get::<RequestId>().is_none() {
                let current_id = current_id.clone();
                response.extensions_mut().insert(RequestId::new(current_id));
            }
        } else if let Some(request_id) = this.request_id.take() {
            response
                .headers_mut()
                .insert(this.header_name.clone(), request_id.0.clone());
            response.extensions_mut().insert(request_id);
        }

        Poll::Ready(Ok(response))
    }
}

/// A [`MakeRequestId`] that generates `UUID`s.
#[derive(Clone, Copy, Default)]
pub struct MakeRequestUuid;

impl MakeRequestId for MakeRequestUuid {
    fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
        let request_id = Uuid::new_v4().to_string().parse().unwrap();
        Some(RequestId::new(request_id))
    }
}

/// Validates a candidate incoming or generated request id in strict mode.
///
/// Used by [`StrictSetRequestId`]. The built-in implementation is [`AsciiRequestId`].
pub trait ValidateRequestId {
    /// Returns `true` if `value` is a legal request id.
    fn validate_request_id(&self, value: &HeaderValue) -> bool;
}

impl<F> ValidateRequestId for F
where
    F: Fn(&HeaderValue) -> bool,
{
    fn validate_request_id(&self, value: &HeaderValue) -> bool {
        self(value)
    }
}

/// A [`ValidateRequestId`] that accepts non-empty values made up of visible ASCII characters
/// (`0x21..=0x7E`, i.e. no spaces or control characters), up to a maximum byte length.
#[derive(Debug, Clone)]
pub struct AsciiRequestId {
    max_len: usize,
}

impl Default for AsciiRequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl AsciiRequestId {
    /// Create a new `AsciiRequestId` with the default maximum length of 128 bytes.
    pub const fn new() -> Self {
        Self { max_len: 128 }
    }

    /// Set the maximum accepted byte length.
    pub fn max_len(mut self, max_len: usize) -> Self {
        self.max_len = max_len;
        self
    }
}

impl ValidateRequestId for AsciiRequestId {
    fn validate_request_id(&self, value: &HeaderValue) -> bool {
        let bytes = value.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= self.max_len
            && bytes.iter().all(|byte| (0x21..=0x7E).contains(byte))
    }
}

/// How [`StrictPropagateRequestId`] treats a response whose request id header differs from the
/// sealed request id.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResponseRequestIdPolicy {
    /// Replace the response header (and extension) with the sealed request id.
    #[default]
    Overwrite,
    /// Reject the response with `500 Internal Server Error` and an empty body.
    Reject,
}

/// Set request id headers in strict mode.
///
/// This layer applies the [`StrictSetRequestId`] middleware. See the [module docs](self) for the
/// rules of strict mode.
#[derive(Debug, Clone)]
pub struct StrictSetRequestIdLayer<M, V = AsciiRequestId> {
    header_name: HeaderName,
    make_request_id: M,
    validator: V,
}

impl<M> StrictSetRequestIdLayer<M, AsciiRequestId> {
    /// Create a new `StrictSetRequestIdLayer` with the default [`AsciiRequestId`] validator.
    pub fn new(header_name: HeaderName, make_request_id: M) -> Self {
        StrictSetRequestIdLayer {
            header_name,
            make_request_id,
            validator: AsciiRequestId::new(),
        }
    }

    /// Create a new `StrictSetRequestIdLayer` that uses `x-request-id` as the header name and the
    /// default [`AsciiRequestId`] validator.
    pub fn x_request_id(make_request_id: M) -> Self {
        Self::new(X_REQUEST_ID, make_request_id)
    }
}

impl<M, V> StrictSetRequestIdLayer<M, V> {
    /// Create a new `StrictSetRequestIdLayer` with a custom [`ValidateRequestId`] implementation.
    pub fn with_validator(header_name: HeaderName, make_request_id: M, validator: V) -> Self {
        StrictSetRequestIdLayer {
            header_name,
            make_request_id,
            validator,
        }
    }

    /// Replace the request id validator.
    pub fn validator<V2>(self, validator: V2) -> StrictSetRequestIdLayer<M, V2> {
        StrictSetRequestIdLayer {
            header_name: self.header_name,
            make_request_id: self.make_request_id,
            validator,
        }
    }
}

impl<M> StrictSetRequestIdLayer<M, AsciiRequestId> {
    /// Use an [`AsciiRequestId`] validator with the given maximum accepted byte length.
    pub fn max_id_len(self, max_len: usize) -> Self {
        self.validator(AsciiRequestId::new().max_len(max_len))
    }
}

impl<S, M, V> Layer<S> for StrictSetRequestIdLayer<M, V>
where
    M: Clone + MakeRequestId,
    V: Clone + ValidateRequestId,
{
    type Service = StrictSetRequestId<S, M, V>;

    fn layer(&self, inner: S) -> Self::Service {
        StrictSetRequestId::with_validator(
            inner,
            self.header_name.clone(),
            self.make_request_id.clone(),
            self.validator.clone(),
        )
    }
}

/// Set request id headers in strict mode.
///
/// See the [module docs](self) for the rules of strict mode.
#[derive(Debug, Clone)]
pub struct StrictSetRequestId<S, M, V = AsciiRequestId> {
    inner: S,
    header_name: HeaderName,
    make_request_id: M,
    validator: V,
}

impl<S, M> StrictSetRequestId<S, M, AsciiRequestId> {
    /// Create a new `StrictSetRequestId` with the default [`AsciiRequestId`] validator.
    pub fn new(inner: S, header_name: HeaderName, make_request_id: M) -> Self {
        Self::with_validator(inner, header_name, make_request_id, AsciiRequestId::new())
    }

    /// Create a new `StrictSetRequestId` that uses `x-request-id` as the header name and the
    /// default [`AsciiRequestId`] validator.
    pub fn x_request_id(inner: S, make_request_id: M) -> Self {
        Self::new(inner, X_REQUEST_ID, make_request_id)
    }
}

impl<S, M, V> StrictSetRequestId<S, M, V> {
    /// Create a new `StrictSetRequestId` with a custom [`ValidateRequestId`] implementation.
    pub fn with_validator(
        inner: S,
        header_name: HeaderName,
        make_request_id: M,
        validator: V,
    ) -> Self {
        Self {
            inner,
            header_name,
            make_request_id,
            validator,
        }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `StrictSetRequestId` middleware.
    pub fn layer(
        header_name: HeaderName,
        make_request_id: M,
        validator: V,
    ) -> StrictSetRequestIdLayer<M, V> {
        StrictSetRequestIdLayer::with_validator(header_name, make_request_id, validator)
    }
}

impl<S, M, ReqBody, ResBody, V> Service<Request<ReqBody>> for StrictSetRequestId<S, M, V>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeRequestId,
    V: ValidateRequestId,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = StrictSetRequestIdFuture<S::Future, ResBody>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // A sealed extension always wins: nesting strict layers must not generate or validate a
        // second identity.
        if req.extensions().get::<RequestId>().is_some() {
            return StrictSetRequestIdFuture::Future {
                inner: self.inner.call(req),
            };
        }

        let reject = |status| StrictSetRequestIdFuture::Reject {
            response: Some(
                Response::builder()
                    .status(status)
                    .body(ResBody::default())
                    .expect("building an empty response cannot fail"),
            ),
        };

        let mut values = req.headers().get_all(&self.header_name).iter();
        let first = values.next();
        let second = values.next();

        let request_id = match (first, second) {
            // Duplicated incoming header: reject before touching the inner service or generator.
            (Some(_), Some(_)) => return reject(StatusCode::BAD_REQUEST),
            (Some(value), None) => {
                let value = value.clone();
                if !self.validator.validate_request_id(&value) {
                    // The illegal value is deliberately not included in the response.
                    return reject(StatusCode::BAD_REQUEST);
                }
                RequestId::new(value)
            }
            // No incoming id: this is the only case in which the generator runs.
            (None, _) => match self.make_request_id.make_request_id(&req) {
                Some(request_id)
                    if self
                        .validator
                        .validate_request_id(request_id.header_value()) =>
                {
                    req.headers_mut()
                        .insert(self.header_name.clone(), request_id.header_value().clone());
                    request_id
                }
                _ => return reject(StatusCode::INTERNAL_SERVER_ERROR),
            },
        };

        req.extensions_mut().insert(request_id);

        StrictSetRequestIdFuture::Future {
            inner: self.inner.call(req),
        }
    }
}

pin_project! {
    /// Response future for [`StrictSetRequestId`].
    #[project = StrictSetRequestIdResponseProj]
    pub enum StrictSetRequestIdFuture<F, B> {
        #[allow(missing_docs)]
        Future {
            #[pin]
            inner: F,
        },
        #[allow(missing_docs)]
        Reject {
            response: Option<Response<B>>,
        },
    }
}

impl<F, B, E> Future for StrictSetRequestIdFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            StrictSetRequestIdResponseProj::Future { inner } => inner.poll(cx),
            StrictSetRequestIdResponseProj::Reject { response } => {
                Poll::Ready(Ok(response.take().expect("future polled after completion")))
            }
        }
    }
}

/// Propagate request ids from requests to responses in strict mode.
///
/// This layer applies the [`StrictPropagateRequestId`] middleware. The response id is always taken
/// from the sealed [`RequestId`] extension, never by re-reading the request header.
#[derive(Debug, Clone)]
pub struct StrictPropagateRequestIdLayer {
    header_name: HeaderName,
    policy: ResponseRequestIdPolicy,
}

impl StrictPropagateRequestIdLayer {
    /// Create a new `StrictPropagateRequestIdLayer` with the default
    /// [`ResponseRequestIdPolicy::Overwrite`] policy.
    pub const fn new(header_name: HeaderName) -> Self {
        Self {
            header_name,
            policy: ResponseRequestIdPolicy::Overwrite,
        }
    }

    /// Create a new `StrictPropagateRequestIdLayer` that uses `x-request-id` as the header name.
    pub const fn x_request_id() -> Self {
        Self::new(X_REQUEST_ID)
    }

    /// Set the policy used when the inner service answers with a different response id.
    pub fn policy(mut self, policy: ResponseRequestIdPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl<S> Layer<S> for StrictPropagateRequestIdLayer {
    type Service = StrictPropagateRequestId<S>;

    fn layer(&self, inner: S) -> Self::Service {
        StrictPropagateRequestId {
            inner,
            header_name: self.header_name.clone(),
            policy: self.policy,
        }
    }
}

/// Propagate request ids from requests to responses in strict mode.
///
/// See the [module docs](self) for the rules of strict mode.
#[derive(Debug, Clone)]
pub struct StrictPropagateRequestId<S> {
    inner: S,
    header_name: HeaderName,
    policy: ResponseRequestIdPolicy,
}

impl<S> StrictPropagateRequestId<S> {
    /// Create a new `StrictPropagateRequestId` with the default
    /// [`ResponseRequestIdPolicy::Overwrite`] policy.
    pub const fn new(inner: S, header_name: HeaderName) -> Self {
        Self {
            inner,
            header_name,
            policy: ResponseRequestIdPolicy::Overwrite,
        }
    }

    /// Create a new `StrictPropagateRequestId` that uses `x-request-id` as the header name.
    pub const fn x_request_id(inner: S) -> Self {
        Self::new(inner, X_REQUEST_ID)
    }

    define_inner_service_accessors!();

    /// Set the policy used when the inner service answers with a different response id.
    pub fn policy(mut self, policy: ResponseRequestIdPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Returns a new [`Layer`] that wraps services with a `StrictPropagateRequestId` middleware.
    pub const fn layer(header_name: HeaderName) -> StrictPropagateRequestIdLayer {
        StrictPropagateRequestIdLayer::new(header_name)
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for StrictPropagateRequestId<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = StrictPropagateRequestIdResponseFuture<S::Future, ResBody>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        // The sealed identity is captured up front; the request header is never read again.
        match req.extensions().get::<RequestId>().cloned() {
            Some(request_id) => StrictPropagateRequestIdResponseFuture::Future {
                inner: self.inner.call(req),
                header_name: self.header_name.clone(),
                request_id,
                policy: self.policy,
            },
            None => StrictPropagateRequestIdResponseFuture::Reject {
                response: Some(
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(ResBody::default())
                        .expect("building an empty response cannot fail"),
                ),
            },
        }
    }
}

pin_project! {
    /// Response future for [`StrictPropagateRequestId`].
    #[project = StrictPropagateRequestIdResponseProj]
    pub enum StrictPropagateRequestIdResponseFuture<F, B> {
        #[allow(missing_docs)]
        Future {
            #[pin]
            inner: F,
            header_name: HeaderName,
            request_id: RequestId,
            policy: ResponseRequestIdPolicy,
        },
        #[allow(missing_docs)]
        Reject {
            response: Option<Response<B>>,
        },
    }
}

impl<F, B, E> Future for StrictPropagateRequestIdResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: Default,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this {
            StrictPropagateRequestIdResponseProj::Reject { response } => {
                Poll::Ready(Ok(response.take().expect("future polled after completion")))
            }
            StrictPropagateRequestIdResponseProj::Future {
                inner,
                header_name,
                request_id,
                policy,
            } => {
                let mut response = ready!(inner.poll(cx))?;

                let sealed_value = request_id.header_value();
                match response.headers().get(&*header_name) {
                    None => {
                        response
                            .headers_mut()
                            .insert(header_name.clone(), sealed_value.clone());
                    }
                    Some(existing) if existing == sealed_value => {}
                    Some(_) => match policy {
                        ResponseRequestIdPolicy::Overwrite => {
                            response
                                .headers_mut()
                                .insert(header_name.clone(), sealed_value.clone());
                        }
                        ResponseRequestIdPolicy::Reject => {
                            return Poll::Ready(Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(B::default())
                                .expect("building an empty response cannot fail")));
                        }
                    },
                }

                // The extension always carries the sealed identity as well.
                response.extensions_mut().insert(request_id.clone());

                Poll::Ready(Ok(response))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_helpers::Body;
    use crate::ServiceBuilderExt as _;
    use http::Response;
    use std::sync::atomic::AtomicUsize;
    use std::{
        convert::Infallible,
        sync::{atomic::Ordering, Arc},
    };
    use tower::{ServiceBuilder, ServiceExt};

    #[allow(unused_imports)]
    use super::*;

    #[tokio::test]
    async fn basic() {
        let svc = ServiceBuilder::new()
            .set_x_request_id(Counter::default())
            .propagate_x_request_id()
            .service_fn(handler);

        // header on response
        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "0");

        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "1");

        // doesn't override if header is already there
        let req = Request::builder()
            .header("x-request-id", "foo")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "foo");

        // extension propagated
        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.extensions().get::<RequestId>().unwrap().0, "2");
    }

    #[tokio::test]
    async fn other_middleware_setting_request_id() {
        let svc = ServiceBuilder::new()
            .override_request_header(
                HeaderName::from_static("x-request-id"),
                HeaderValue::from_str("foo").unwrap(),
            )
            .set_x_request_id(Counter::default())
            .map_request(|request: Request<_>| {
                // `set_x_request_id` should set the extension if its missing
                assert_eq!(request.extensions().get::<RequestId>().unwrap().0, "foo");
                request
            })
            .propagate_x_request_id()
            .service_fn(handler);

        let req = Request::builder()
            .header(
                "x-request-id",
                "this-will-be-overriden-by-override_request_header-middleware",
            )
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "foo");
        assert_eq!(res.extensions().get::<RequestId>().unwrap().0, "foo");
    }

    #[tokio::test]
    async fn other_middleware_setting_request_id_on_response() {
        let svc = ServiceBuilder::new()
            .set_x_request_id(Counter::default())
            .propagate_x_request_id()
            .override_response_header(
                HeaderName::from_static("x-request-id"),
                HeaderValue::from_str("foo").unwrap(),
            )
            .service_fn(handler);

        let req = Request::builder()
            .header("x-request-id", "foo")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "foo");
        assert_eq!(res.extensions().get::<RequestId>().unwrap().0, "foo");
    }

    #[derive(Clone, Default)]
    struct Counter(Arc<AtomicUsize>);

    impl MakeRequestId for Counter {
        fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
            let id =
                HeaderValue::from_str(&self.0.fetch_add(1, Ordering::SeqCst).to_string()).unwrap();
            Some(RequestId::new(id))
        }
    }

    async fn handler(_: Request<Body>) -> Result<Response<Body>, Infallible> {
        Ok(Response::new(Body::empty()))
    }

    #[tokio::test]
    async fn uuid() {
        let svc = ServiceBuilder::new()
            .set_x_request_id(MakeRequestUuid)
            .propagate_x_request_id()
            .service_fn(handler);

        // header on response
        let req = Request::builder().body(Body::empty()).unwrap();
        let mut res = svc.clone().oneshot(req).await.unwrap();
        let id = res.headers_mut().remove("x-request-id").unwrap();
        id.to_str().unwrap().parse::<Uuid>().unwrap();
    }

    #[derive(Clone)]
    struct CountingGenerator {
        calls: Arc<AtomicUsize>,
        value: HeaderValue,
    }

    impl Default for CountingGenerator {
        fn default() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                value: HeaderValue::from_static("generated-7"),
            }
        }
    }

    impl MakeRequestId for CountingGenerator {
        fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(RequestId::new(self.value.clone()))
        }
    }

    fn strict_svc(
        generator: CountingGenerator,
        policy: ResponseRequestIdPolicy,
        service_calls: Arc<AtomicUsize>,
    ) -> impl tower::Service<
        Request<Body>,
        Response = Response<Body>,
        Error = Infallible,
        Future: Send,
    > + Clone {
        ServiceBuilder::new()
            .layer(StrictSetRequestIdLayer::x_request_id(generator))
            .layer(StrictPropagateRequestIdLayer::x_request_id().policy(policy))
            .service_fn(move |req| {
                let service_calls = service_calls.clone();
                async move { strict_handler(req, &service_calls).await }
            })
    }

    // Inner service that records how often it ran and mutates the request header to prove the
    // sealed extension is what gets propagated.
    async fn strict_handler(
        mut req: Request<Body>,
        service_calls: &AtomicUsize,
    ) -> Result<Response<Body>, Infallible> {
        service_calls.fetch_add(1, Ordering::SeqCst);
        req.headers_mut().insert(
            X_REQUEST_ID.clone(),
            HeaderValue::from_static("mutated-by-service"),
        );
        Ok(Response::new(Body::empty()))
    }

    #[tokio::test]
    async fn strict_mode() {
        let service_calls = Arc::new(AtomicUsize::new(0));
        let generator = CountingGenerator::default();
        let generator_calls = generator.calls.clone();
        let svc = strict_svc(
            generator,
            ResponseRequestIdPolicy::Overwrite,
            service_calls.clone(),
        );

        // 1. a legal incoming id is echoed back verbatim
        let req = Request::builder()
            .header("x-request-id", "alpha-123")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-request-id"], "alpha-123");
        assert_eq!(res.extensions().get::<RequestId>().unwrap().0, "alpha-123");

        // 2. a duplicated incoming id is rejected: no inner service, no generator
        let req = Request::builder()
            .header("x-request-id", "alpha-123")
            .header("x-request-id", "beta-456")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(res.headers().get("x-request-id").is_none());

        // 3. an illegal incoming id is rejected as well, and never echoed
        let req = Request::builder()
            .header("x-request-id", "bad value")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(res.headers().get("x-request-id").is_none());

        // only the legal request reached the inner service; the generator never ran
        assert_eq!(service_calls.load(Ordering::SeqCst), 1);
        assert_eq!(generator_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn strict_generator_only_runs_without_id_and_result_is_sealed() {
        let service_calls = Arc::new(AtomicUsize::new(0));
        let generator = CountingGenerator {
            calls: Arc::new(AtomicUsize::new(0)),
            value: HeaderValue::from_static("generated-7"),
        };
        let generator_calls = generator.calls.clone();
        let svc = strict_svc(
            generator,
            ResponseRequestIdPolicy::Overwrite,
            service_calls.clone(),
        );

        // missing id: the generator runs once and the service mutation does not change the seal
        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-request-id"], "generated-7");
        assert_eq!(
            res.extensions().get::<RequestId>().unwrap().0,
            "generated-7"
        );
        assert_eq!(generator_calls.load(Ordering::SeqCst), 1);

        // present legal id: generator is not consulted again
        let req = Request::builder()
            .header("x-request-id", "incoming-9")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "incoming-9");
        assert_eq!(generator_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn strict_invalid_generated_id_is_rejected() {
        let service_calls = Arc::new(AtomicUsize::new(0));
        let service_calls_for_handler = service_calls.clone();

        #[derive(Clone)]
        struct IllegalGenerator {
            calls: Arc<AtomicUsize>,
        }

        impl MakeRequestId for IllegalGenerator {
            fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Some(RequestId::new(HeaderValue::from_static("not legal")))
            }
        }

        let generator = IllegalGenerator {
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let calls = generator.calls.clone();
        let svc = ServiceBuilder::new()
            .layer(StrictSetRequestIdLayer::x_request_id(generator))
            .layer(StrictPropagateRequestIdLayer::x_request_id())
            .service_fn(move |req| {
                let service_calls = service_calls_for_handler.clone();
                async move { strict_handler(req, &service_calls).await }
            });

        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(res.headers().get("x-request-id").is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // the inner service must not run when the generated id is illegal
        assert_eq!(service_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn strict_response_id_policies() {
        async fn handler_setting_response_id(
            _: Request<Body>,
        ) -> Result<Response<Body>, Infallible> {
            let mut res = Response::new(Body::empty());
            res.headers_mut().insert(
                X_REQUEST_ID.clone(),
                HeaderValue::from_static("from-the-service"),
            );
            Ok(res)
        }

        let stack = |policy| {
            ServiceBuilder::new()
                .layer(StrictSetRequestIdLayer::x_request_id(CountingGenerator {
                    calls: Arc::new(AtomicUsize::new(0)),
                    value: HeaderValue::from_static("sealed-id"),
                }))
                .layer(StrictPropagateRequestIdLayer::x_request_id().policy(policy))
                .service_fn(handler_setting_response_id)
        };

        // overwrite: the differing response id is replaced with the sealed identity
        let req = Request::builder()
            .header("x-request-id", "sealed-id")
            .body(Body::empty())
            .unwrap();
        let res = stack(ResponseRequestIdPolicy::Overwrite)
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-request-id"], "sealed-id");

        // reject: a differing response id turns into a 500
        let req = Request::builder()
            .header("x-request-id", "sealed-id")
            .body(Body::empty())
            .unwrap();
        let res = stack(ResponseRequestIdPolicy::Reject)
            .oneshot(req)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(res.headers().get("x-request-id").is_none());

        // a matching response id is fine even with the reject policy
        async fn echo_handler(req: Request<Body>) -> Result<Response<Body>, Infallible> {
            let mut res = Response::new(Body::empty());
            let id = req
                .extensions()
                .get::<RequestId>()
                .unwrap()
                .header_value()
                .clone();
            res.headers_mut().insert(X_REQUEST_ID.clone(), id);
            Ok(res)
        }
        let svc = ServiceBuilder::new()
            .layer(StrictSetRequestIdLayer::x_request_id(
                CountingGenerator::default(),
            ))
            .layer(
                StrictPropagateRequestIdLayer::x_request_id()
                    .policy(ResponseRequestIdPolicy::Reject),
            )
            .service_fn(echo_handler);
        let req = Request::builder()
            .header("x-request-id", "same-id")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-request-id"], "same-id");
    }

    #[tokio::test]
    async fn strict_propagate_without_sealed_extension_is_rejected() {
        // a strict propagate layer in front of a request that never went through a set layer
        let svc = ServiceBuilder::new()
            .layer(StrictPropagateRequestIdLayer::x_request_id())
            .service_fn(handler);

        let req = Request::builder()
            .header("x-request-id", "alpha-123")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn nested_strict_layers_do_not_regenerate() {
        let outer_generator = CountingGenerator {
            calls: Arc::new(AtomicUsize::new(0)),
            value: HeaderValue::from_static("outer-generated"),
        };
        let outer_calls = outer_generator.calls.clone();

        let inner_generator = CountingGenerator {
            calls: Arc::new(AtomicUsize::new(0)),
            value: HeaderValue::from_static("inner-generated"),
        };
        let inner_calls = inner_generator.calls.clone();

        let svc = ServiceBuilder::new()
            // added first: outermost
            .layer(StrictSetRequestIdLayer::x_request_id(outer_generator))
            // added second: innermost; must reuse the seal instead of regenerating
            .layer(StrictSetRequestIdLayer::x_request_id(inner_generator))
            .layer(StrictPropagateRequestIdLayer::x_request_id())
            .service_fn(handler);

        // no incoming id: only the outer generator runs; the inner layer reuses the seal
        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "outer-generated");
        assert_eq!(outer_calls.load(Ordering::SeqCst), 1);
        assert_eq!(inner_calls.load(Ordering::SeqCst), 0);

        // a legal incoming id survives both layers untouched, with no generation at all
        let req = Request::builder()
            .header("x-request-id", "nested-incoming")
            .body(Body::empty())
            .unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "nested-incoming");
        assert_eq!(outer_calls.load(Ordering::SeqCst), 1);
        assert_eq!(inner_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn strict_uuid_generator_output_is_accepted() {
        let svc = ServiceBuilder::new()
            .layer(StrictSetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(StrictPropagateRequestIdLayer::x_request_id())
            .service_fn(handler);

        let req = Request::builder().body(Body::empty()).unwrap();
        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        res.headers()["x-request-id"]
            .to_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap();
    }

    #[tokio::test]
    async fn default_layers_keep_their_permissive_behavior() {
        let generator = CountingGenerator::default();
        let svc = ServiceBuilder::new()
            .set_x_request_id(generator)
            .propagate_x_request_id()
            .service_fn(handler);

        // duplicates and "illegal" values are not rejected by the default layers
        let req = Request::builder()
            .header("x-request-id", "one")
            .header("x-request-id", "two")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let req = Request::builder()
            .header("x-request-id", "bad value")
            .body(Body::empty())
            .unwrap();
        let res = svc.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["x-request-id"], "bad value");
    }
}
