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
//! The default layers are permissive: they accept any header value and never reject a request.
//! For services that require a stable, trustworthy request identity, strict mode is opt-in via
//! [`SetRequestIdLayer::strict`] and [`PropagateRequestIdLayer::strict`] (or the
//! `set_request_id_strict` / `propagate_request_id_strict` methods on [`ServiceBuilderExt`]).
//!
//! In strict mode:
//!
//! - duplicate or malformed incoming ids are rejected before the inner service is called;
//! - the generator is only invoked when no id is present, and its output is validated;
//! - a legal id is sealed into the request extension and the sealed identity is authoritative;
//! - the response is propagated using the sealed identity, without re-reading headers that the
//!   service may have modified;
//! - a response that carries a different id is rejected or overwritten according to the
//!   [`ResponseIdPolicy`].
//!
//! Error messages never contain the offending header value. The default (non-strict) behavior is
//! unchanged.
//!
//! ```
//! use http::{Request, Response, header::HeaderName};
//! use tower::{Service, ServiceExt, ServiceBuilder};
//! use tower_http::request_id::{
//!     SetRequestIdLayer, PropagateRequestIdLayer, MakeRequestId, RequestId,
//! };
//! use http_body_util::Full;
//! use bytes::Bytes;
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let handler = tower::service_fn(|request: Request<Full<Bytes>>| async move {
//! #     Ok::<_, std::convert::Infallible>(Response::new(request.into_body()))
//! # });
//! # let make_request_id = tower_http::request_id::MakeRequestUuid;
//! let mut svc = ServiceBuilder::new()
//!     .layer(SetRequestIdLayer::x_request_id(make_request_id).strict())
//!     .layer(PropagateRequestIdLayer::x_request_id().strict())
//!     .service(handler);
//!
//! // A legal incoming id is echoed back as-is.
//! let request = Request::builder()
//!     .header("x-request-id", "req-abc-123")
//!     .body(Full::default())
//!     .unwrap();
//! let response = svc.ready().await?.call(request).await?;
//! assert_eq!(response.headers()["x-request-id"], "req-abc-123");
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
    Request, Response,
};
use pin_project_lite::pin_project;
use std::task::{ready, Context, Poll};
use std::{error::Error as StdError, fmt};
use std::{future::Future, pin::Pin};
use tower_layer::Layer;
use tower_service::Service;
use uuid::Uuid;

pub(crate) const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Returns `true` if `value` is a legal request id in strict mode.
///
/// A legal request id is non-empty and contains only visible ASCII characters
/// (`0x21..=0x7e`). This rejects empty values as well as values containing
/// spaces, tabs, control characters or non-ASCII bytes.
fn is_valid_request_id(value: &HeaderValue) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty() && bytes.iter().all(|&b| (0x21..=0x7e).contains(&b))
}

/// A violation detected by strict request-id middleware.
///
/// Strict mode rejects malformed or inconsistent request ids instead of silently
/// falling back. The error messages never contain the offending header value, so
/// untrusted input is not reflected back to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequestIdError {
    /// The request carried more than one request-id header.
    DuplicateRequestId,
    /// The incoming request-id header had an illegal format.
    InvalidRequestId,
    /// No request-id was present and the generator returned `None`.
    MissingRequestId,
    /// The generator produced an illegal request-id.
    InvalidGeneratedRequestId,
    /// The response carried a request-id different from the sealed identity.
    ResponseIdMismatch,
}

impl fmt::Display for RequestIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateRequestId => write!(f, "duplicate request-id header"),
            Self::InvalidRequestId => write!(f, "invalid request-id header"),
            Self::MissingRequestId => write!(f, "missing request-id"),
            Self::InvalidGeneratedRequestId => write!(f, "generated request-id is invalid"),
            Self::ResponseIdMismatch => {
                write!(f, "response request-id does not match the sealed identity")
            }
        }
    }
}

impl StdError for RequestIdError {}

/// What to do when a response carries a request id different from the sealed
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ResponseIdPolicy {
    /// Reject the response with an error.
    ///
    /// This is the default.
    #[default]
    Reject,
    /// Overwrite the response header (and extension) with the sealed identity.
    Overwrite,
}

/// The error type for strict request-id middleware.
///
/// Errors from the inner service are wrapped in [`Inner`](Self::Inner), while
/// request-id violations are wrapped in [`RequestId`](Self::RequestId).
#[derive(Debug)]
#[non_exhaustive]
pub enum RequestIdServiceError<E> {
    /// The inner service returned an error.
    Inner(E),
    /// A request-id violation was detected.
    RequestId(RequestIdError),
}

impl<E: fmt::Display> fmt::Display for RequestIdServiceError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inner(e) => write!(f, "{e}"),
            Self::RequestId(e) => write!(f, "{e}"),
        }
    }
}

impl<E: StdError + 'static> StdError for RequestIdServiceError<E> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Inner(e) => Some(e),
            Self::RequestId(e) => Some(e),
        }
    }
}

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

    /// Switch to strict mode.
    ///
    /// In strict mode:
    ///
    /// - duplicate or malformed incoming request ids are rejected before the
    ///   inner service is called;
    /// - the generator is only invoked when no id is present, and its output is
    ///   validated;
    /// - a legal id is sealed into the request extension and the sealed identity
    ///   is authoritative.
    ///
    /// The default (non-strict) behavior is unchanged.
    pub fn strict(self) -> StrictSetRequestIdLayer<M> {
        StrictSetRequestIdLayer {
            header_name: self.header_name,
            make_request_id: self.make_request_id,
        }
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

    /// Switch to strict mode.
    ///
    /// In strict mode the sealed request identity is propagated to the response
    /// without re-reading headers that the service may have modified. See
    /// [`StrictPropagateRequestIdLayer`] for details.
    pub fn strict(self) -> StrictPropagateRequestIdLayer {
        StrictPropagateRequestIdLayer {
            header_name: self.header_name,
            policy: ResponseIdPolicy::Reject,
        }
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

/// A [`Layer`] that applies [`StrictSetRequestId`].
///
/// See the [module docs](self) and [`StrictSetRequestId`] for more details.
#[derive(Debug, Clone)]
pub struct StrictSetRequestIdLayer<M> {
    header_name: HeaderName,
    make_request_id: M,
}

impl<M> StrictSetRequestIdLayer<M> {
    /// Create a new `StrictSetRequestIdLayer`.
    pub const fn new(header_name: HeaderName, make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        Self {
            header_name,
            make_request_id,
        }
    }

    /// Create a new `StrictSetRequestIdLayer` that uses `x-request-id` as the header name.
    pub const fn x_request_id(make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        Self::new(X_REQUEST_ID, make_request_id)
    }
}

impl<S, M> Layer<S> for StrictSetRequestIdLayer<M>
where
    M: Clone + MakeRequestId,
{
    type Service = StrictSetRequestId<S, M>;

    fn layer(&self, inner: S) -> Self::Service {
        StrictSetRequestId::new(
            inner,
            self.header_name.clone(),
            self.make_request_id.clone(),
        )
    }
}

/// Set request id headers and extensions on requests, rejecting malformed ids.
///
/// This is the strict counterpart of [`SetRequestId`].
///
/// # Behavior
///
/// - If the request already carries a sealed [`RequestId`] extension (e.g. an
///   outer layer already sealed one), it is reused and the generator is never
///   invoked again. The header is restored to the sealed identity.
/// - If the request carries the header more than once, the request is rejected
///   with [`RequestIdError::DuplicateRequestId`] and the inner service is never
///   called.
/// - If the request carries a single illegal header value, it is rejected with
///   [`RequestIdError::InvalidRequestId`] and the inner service is never called.
/// - If the header is absent, [`MakeRequestId::make_request_id`] is called
///   exactly once. A `None` result is rejected with
///   [`RequestIdError::MissingRequestId`], and an illegal generated value is
///   rejected with [`RequestIdError::InvalidGeneratedRequestId`].
/// - A legal id is inserted into [`Request::extensions`] and the request header.
///
/// Error messages never contain the offending header value.
#[derive(Debug, Clone)]
pub struct StrictSetRequestId<S, M> {
    inner: S,
    header_name: HeaderName,
    make_request_id: M,
}

impl<S, M> StrictSetRequestId<S, M> {
    /// Create a new `StrictSetRequestId`.
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

    /// Create a new `StrictSetRequestId` that uses `x-request-id` as the header name.
    pub const fn x_request_id(inner: S, make_request_id: M) -> Self
    where
        M: MakeRequestId,
    {
        Self::new(inner, X_REQUEST_ID, make_request_id)
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `StrictSetRequestId` middleware.
    pub const fn layer(header_name: HeaderName, make_request_id: M) -> StrictSetRequestIdLayer<M>
    where
        M: MakeRequestId,
    {
        StrictSetRequestIdLayer::new(header_name, make_request_id)
    }
}

impl<S, M, ReqBody, ResBody> Service<Request<ReqBody>> for StrictSetRequestId<S, M>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    M: MakeRequestId,
{
    type Response = S::Response;
    type Error = RequestIdServiceError<S::Error>;
    type Future = StrictSetRequestIdResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestIdServiceError::Inner)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // If an outer layer already sealed an identity, reuse it. This guarantees
        // nested layers never re-generate an id.
        if let Some(sealed) = req.extensions().get::<RequestId>().cloned() {
            req.headers_mut()
                .insert(self.header_name.clone(), sealed.into_header_value());
            return StrictSetRequestIdResponseFuture {
                inner: Some(self.inner.call(req)),
                rejection: None,
            };
        }

        let mut values = req.headers().get_all(&self.header_name).iter();
        let first = values.next();
        let second = values.next();

        match (first, second) {
            (None, _) => {
                // No incoming id: the generator is called exactly once.
                match self.make_request_id.make_request_id(&req) {
                    None => StrictSetRequestIdResponseFuture {
                        inner: None,
                        rejection: Some(RequestIdError::MissingRequestId),
                    },
                    Some(id) if !is_valid_request_id(id.header_value()) => {
                        StrictSetRequestIdResponseFuture {
                            inner: None,
                            rejection: Some(RequestIdError::InvalidGeneratedRequestId),
                        }
                    }
                    Some(id) => {
                        req.extensions_mut().insert(id.clone());
                        req.headers_mut()
                            .insert(self.header_name.clone(), id.into_header_value());
                        StrictSetRequestIdResponseFuture {
                            inner: Some(self.inner.call(req)),
                            rejection: None,
                        }
                    }
                }
            }
            (Some(_), Some(_)) => StrictSetRequestIdResponseFuture {
                inner: None,
                rejection: Some(RequestIdError::DuplicateRequestId),
            },
            (Some(value), None) => {
                if !is_valid_request_id(value) {
                    StrictSetRequestIdResponseFuture {
                        inner: None,
                        rejection: Some(RequestIdError::InvalidRequestId),
                    }
                } else {
                    let id = RequestId::new(value.clone());
                    req.extensions_mut().insert(id);
                    StrictSetRequestIdResponseFuture {
                        inner: Some(self.inner.call(req)),
                        rejection: None,
                    }
                }
            }
        }
    }
}

pin_project! {
    /// Response future for [`StrictSetRequestId`].
    pub struct StrictSetRequestIdResponseFuture<F> {
        #[pin]
        inner: Option<F>,
        rejection: Option<RequestIdError>,
    }
}

impl<F, B, E> Future for StrictSetRequestIdResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, RequestIdServiceError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        if let Some(rejection) = this.rejection.take() {
            return Poll::Ready(Err(RequestIdServiceError::RequestId(rejection)));
        }
        match this.inner.as_pin_mut() {
            Some(future) => future
                .poll(cx)
                .map(|result| result.map_err(RequestIdServiceError::Inner)),
            None => Poll::Pending,
        }
    }
}

/// A [`Layer`] that applies [`StrictPropagateRequestId`].
///
/// See the [module docs](self) and [`StrictPropagateRequestId`] for more details.
#[derive(Debug, Clone)]
pub struct StrictPropagateRequestIdLayer {
    header_name: HeaderName,
    policy: ResponseIdPolicy,
}

impl StrictPropagateRequestIdLayer {
    /// Create a new `StrictPropagateRequestIdLayer` with the default
    /// [`ResponseIdPolicy::Reject`] policy.
    pub const fn new(header_name: HeaderName) -> Self {
        Self {
            header_name,
            policy: ResponseIdPolicy::Reject,
        }
    }

    /// Create a new `StrictPropagateRequestIdLayer` that uses `x-request-id` as the header name.
    pub const fn x_request_id() -> Self {
        Self::new(X_REQUEST_ID)
    }

    /// Set the policy used when the response carries a different request id.
    pub fn with_policy(mut self, policy: ResponseIdPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Reject responses that carry a different request id.
    pub fn reject_on_mismatch(self) -> Self {
        self.with_policy(ResponseIdPolicy::Reject)
    }

    /// Overwrite responses that carry a different request id with the sealed identity.
    pub fn overwrite_on_mismatch(self) -> Self {
        self.with_policy(ResponseIdPolicy::Overwrite)
    }
}

impl<S> Layer<S> for StrictPropagateRequestIdLayer {
    type Service = StrictPropagateRequestId<S>;

    fn layer(&self, inner: S) -> Self::Service {
        StrictPropagateRequestId::new(inner, self.header_name.clone(), self.policy)
    }
}

/// Propagate the sealed request identity to responses, rejecting mismatches.
///
/// This is the strict counterpart of [`PropagateRequestId`].
///
/// The sealed identity is captured from the request extension (or the incoming
/// header) **before** the inner service is called. The request header is never
/// re-read after the service runs, so a service that modifies the request header
/// cannot change the propagated identity.
///
/// Behavior on the response:
///
/// - no response header: the sealed identity is inserted;
/// - a single response header equal to the sealed identity: it is kept and the
///   extension is sealed;
/// - a single response header different from the sealed identity, or multiple
///   response headers: the response is rejected with
///   [`RequestIdError::ResponseIdMismatch`] under [`ResponseIdPolicy::Reject`],
///   or overwritten with the sealed identity under
///   [`ResponseIdPolicy::Overwrite`].
#[derive(Debug, Clone)]
pub struct StrictPropagateRequestId<S> {
    inner: S,
    header_name: HeaderName,
    policy: ResponseIdPolicy,
}

impl<S> StrictPropagateRequestId<S> {
    /// Create a new `StrictPropagateRequestId` with the default
    /// [`ResponseIdPolicy::Reject`] policy.
    pub const fn new(inner: S, header_name: HeaderName, policy: ResponseIdPolicy) -> Self {
        Self {
            inner,
            header_name,
            policy,
        }
    }

    /// Create a new `StrictPropagateRequestId` that uses `x-request-id` as the header name.
    pub const fn x_request_id(inner: S) -> Self {
        Self::new(inner, X_REQUEST_ID, ResponseIdPolicy::Reject)
    }

    /// Set the policy used when the response carries a different request id.
    pub fn with_policy(mut self, policy: ResponseIdPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Reject responses that carry a different request id.
    pub fn reject_on_mismatch(self) -> Self {
        self.with_policy(ResponseIdPolicy::Reject)
    }

    /// Overwrite responses that carry a different request id with the sealed identity.
    pub fn overwrite_on_mismatch(self) -> Self {
        self.with_policy(ResponseIdPolicy::Overwrite)
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `StrictPropagateRequestId` middleware.
    pub const fn layer(header_name: HeaderName) -> StrictPropagateRequestIdLayer {
        StrictPropagateRequestIdLayer::new(header_name)
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for StrictPropagateRequestId<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = RequestIdServiceError<S::Error>;
    type Future = StrictPropagateRequestIdResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestIdServiceError::Inner)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Capture the sealed identity BEFORE calling the inner service. Prefer
        // the sealed extension, falling back to the incoming header. The header
        // is never re-read after the service runs.
        let sealed = req
            .extensions()
            .get::<RequestId>()
            .cloned()
            .or_else(|| {
                req.headers()
                    .get(&self.header_name)
                    .cloned()
                    .map(RequestId::new)
            });

        if let Some(id) = &sealed {
            if req.extensions().get::<RequestId>().is_none() {
                req.extensions_mut().insert(id.clone());
            }
        }

        StrictPropagateRequestIdResponseFuture {
            inner: self.inner.call(req),
            header_name: self.header_name.clone(),
            sealed,
            policy: self.policy,
        }
    }
}

pin_project! {
    /// Response future for [`StrictPropagateRequestId`].
    pub struct StrictPropagateRequestIdResponseFuture<F> {
        #[pin]
        inner: F,
        header_name: HeaderName,
        sealed: Option<RequestId>,
        policy: ResponseIdPolicy,
    }
}

impl<F, B, E> Future for StrictPropagateRequestIdResponseFuture<F>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, RequestIdServiceError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut response = ready!(this.inner.poll(cx)).map_err(RequestIdServiceError::Inner)?;

        if let Some(sealed) = this.sealed {
            let mut values = response.headers().get_all(&*this.header_name).iter();
            let first = values.next();
            let second = values.next();

            match (first, second) {
                (None, _) => {
                    response
                        .headers_mut()
                        .insert(this.header_name.clone(), sealed.header_value().clone());
                    response.extensions_mut().insert(sealed.clone());
                }
                (Some(value), None) if value == sealed.header_value() => {
                    if response.extensions().get::<RequestId>().is_none() {
                        response.extensions_mut().insert(sealed.clone());
                    }
                }
                _ => match this.policy {
                    ResponseIdPolicy::Reject => {
                        return Poll::Ready(Err(RequestIdServiceError::RequestId(
                            RequestIdError::ResponseIdMismatch,
                        )));
                    }
                    ResponseIdPolicy::Overwrite => {
                        response.headers_mut().insert(
                            this.header_name.clone(),
                            sealed.header_value().clone(),
                        );
                        response.extensions_mut().insert(sealed.clone());
                    }
                },
            }
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

    // ---- strict mode tests ----

    #[derive(Clone)]
    struct CountingGenerator {
        calls: Arc<AtomicUsize>,
        value: HeaderValue,
    }

    impl CountingGenerator {
        fn legal(calls: Arc<AtomicUsize>) -> Self {
            Self {
                calls,
                value: HeaderValue::from_static("generated-1"),
            }
        }

        fn illegal(calls: Arc<AtomicUsize>) -> Self {
            Self {
                calls,
                value: HeaderValue::from_static(""),
            }
        }
    }

    impl MakeRequestId for CountingGenerator {
        fn make_request_id<B>(&mut self, _request: &Request<B>) -> Option<RequestId> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(RequestId::new(self.value.clone()))
        }
    }

    fn counting_handler(
        calls: Arc<AtomicUsize>,
    ) -> impl FnMut(Request<Body>) -> futures_util::future::BoxFuture<'static, Result<Response<Body>, Infallible>>
           + Clone {
        move |_req: Request<Body>| {
            let calls = calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, Infallible>(Response::new(Body::empty()))
            })
        }
    }

    #[tokio::test]
    async fn strict_duplicate_incoming_rejected() {
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let generator_calls = Arc::new(AtomicUsize::new(0));

        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(generator_calls.clone()))
            .propagate_x_request_id_strict()
            .service_fn(counting_handler(handler_calls.clone()));

        let req = Request::builder()
            .header("x-request-id", "a")
            .header("x-request-id", "b")
            .body(Body::empty())
            .unwrap();

        let err = svc.oneshot(req).await.unwrap_err();
        assert!(matches!(
            err,
            RequestIdServiceError::RequestId(RequestIdError::DuplicateRequestId)
        ));
        assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
        assert_eq!(generator_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn strict_illegal_incoming_rejected() {
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let generator_calls = Arc::new(AtomicUsize::new(0));

        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(generator_calls.clone()))
            .propagate_x_request_id_strict()
            .service_fn(counting_handler(handler_calls.clone()));

        let req = Request::builder()
            .header("x-request-id", "not legal")
            .body(Body::empty())
            .unwrap();

        let err = svc.oneshot(req).await.unwrap_err();
        assert!(matches!(
            err,
            RequestIdServiceError::RequestId(RequestIdError::InvalidRequestId)
        ));
        assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
        assert_eq!(generator_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn strict_legal_incoming_echoed() {
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let generator_calls = Arc::new(AtomicUsize::new(0));

        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(generator_calls.clone()))
            .propagate_x_request_id_strict()
            .service_fn(counting_handler(handler_calls.clone()));

        let req = Request::builder()
            .header("x-request-id", "req-abc-123")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "req-abc-123");
        assert_eq!(
            res.extensions().get::<RequestId>().unwrap().header_value(),
            "req-abc-123"
        );
        assert_eq!(handler_calls.load(Ordering::SeqCst), 1);
        assert_eq!(generator_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn strict_missing_id_generated() {
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let generator_calls = Arc::new(AtomicUsize::new(0));

        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(generator_calls.clone()))
            .propagate_x_request_id_strict()
            .service_fn(counting_handler(handler_calls.clone()));

        let req = Request::builder().body(Body::empty()).unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "generated-1");
        assert_eq!(
            res.extensions().get::<RequestId>().unwrap().header_value(),
            "generated-1"
        );
        assert_eq!(handler_calls.load(Ordering::SeqCst), 1);
        assert_eq!(generator_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn strict_generated_illegal_rejected() {
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let generator_calls = Arc::new(AtomicUsize::new(0));

        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::illegal(generator_calls.clone()))
            .propagate_x_request_id_strict()
            .service_fn(counting_handler(handler_calls.clone()));

        let req = Request::builder().body(Body::empty()).unwrap();

        let err = svc.oneshot(req).await.unwrap_err();
        assert!(matches!(
            err,
            RequestIdServiceError::RequestId(RequestIdError::InvalidGeneratedRequestId)
        ));
        assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
        assert_eq!(generator_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn strict_service_modifies_request_header_uses_sealed_identity() {
        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(Arc::new(AtomicUsize::new(0))))
            .propagate_x_request_id_strict()
            .service_fn(|mut req: Request<Body>| async move {
                // The service tampers with the request header after it was sealed.
                req.headers_mut()
                    .insert("x-request-id", "tampered".parse().unwrap());
                Ok::<_, Infallible>(Response::new(Body::empty()))
            });

        let req = Request::builder()
            .header("x-request-id", "sealed-id")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        // The response must carry the sealed identity, not the tampered header.
        assert_eq!(res.headers()["x-request-id"], "sealed-id");
        assert_eq!(
            res.extensions().get::<RequestId>().unwrap().header_value(),
            "sealed-id"
        );
    }

    #[tokio::test]
    async fn strict_nested_layers_do_not_regenerate() {
        let generator_calls = Arc::new(AtomicUsize::new(0));

        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(generator_calls.clone()))
            .set_x_request_id_strict(CountingGenerator::legal(generator_calls.clone()))
            .propagate_x_request_id_strict()
            .service_fn(handler);

        let req = Request::builder().body(Body::empty()).unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "generated-1");
        assert_eq!(generator_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn strict_response_mismatch_rejected() {
        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(Arc::new(AtomicUsize::new(0))))
            .propagate_x_request_id_strict()
            .service_fn(|_req: Request<Body>| async move {
                let mut res = Response::new(Body::empty());
                res.headers_mut()
                    .insert("x-request-id", "different".parse().unwrap());
                Ok::<_, Infallible>(res)
            });

        let req = Request::builder()
            .header("x-request-id", "sealed-id")
            .body(Body::empty())
            .unwrap();

        let err = svc.oneshot(req).await.unwrap_err();
        assert!(matches!(
            err,
            RequestIdServiceError::Inner(RequestIdServiceError::RequestId(
                RequestIdError::ResponseIdMismatch
            ))
        ));
    }

    #[tokio::test]
    async fn strict_response_mismatch_overwritten() {
        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(Arc::new(AtomicUsize::new(0))))
            .layer(StrictPropagateRequestIdLayer::x_request_id().overwrite_on_mismatch())
            .service_fn(|_req: Request<Body>| async move {
                let mut res = Response::new(Body::empty());
                res.headers_mut()
                    .insert("x-request-id", "different".parse().unwrap());
                Ok::<_, Infallible>(res)
            });

        let req = Request::builder()
            .header("x-request-id", "sealed-id")
            .body(Body::empty())
            .unwrap();

        let res = svc.oneshot(req).await.unwrap();
        assert_eq!(res.headers()["x-request-id"], "sealed-id");
        assert_eq!(
            res.extensions().get::<RequestId>().unwrap().header_value(),
            "sealed-id"
        );
    }

    #[tokio::test]
    async fn strict_error_does_not_contain_illegal_value() {
        let svc = ServiceBuilder::new()
            .set_x_request_id_strict(CountingGenerator::legal(Arc::new(AtomicUsize::new(0))))
            .propagate_x_request_id_strict()
            .service_fn(handler);

        let req = Request::builder()
            .header("x-request-id", "has spaces")
            .body(Body::empty())
            .unwrap();

        let err = svc.oneshot(req).await.unwrap_err();
        let message = err.to_string();
        assert!(!message.contains("has spaces"));
    }
}
