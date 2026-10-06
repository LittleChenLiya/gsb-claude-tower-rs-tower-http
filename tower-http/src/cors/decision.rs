//! Explicit, sealed CORS policy decisions.
//!
//! [`CorsPolicy`] is an immutable snapshot ("sealed policy") of a
//! [`CorsLayer`] configuration. Sealing validates the configuration up front
//! (most notably the forbidden `Access-Control-Allow-Credentials: true` +
//! wildcard combination) and produces a single policy object that is then
//! shared, unchanged, by preflight and actual requests alike.
//!
//! Each request is evaluated exactly once into an explicit [`Decision`]
//! snapshot. Dynamic policies (origin/credentials/max-age predicates) run at
//! most once per request; the resulting decision is reused for the whole
//! request/response cycle. Denied requests are answered with `403 Forbidden`
//! without ever executing the inner service, and an unknown origin is never
//! echoed back in `Access-Control-Allow-Origin`.
//!
//! ## Policy table
//!
//! | Situation | Outcome |
//! |---|---|
//! | `allow_credentials(true)` + wildcard origin, methods, headers or expose-headers | rejected when sealing: `Err(InvalidCorsPolicy)` |
//! | credentials allowed (via predicate) while the origin policy is a wildcard | `403`, [`DenyReason::CredentialsWithWildcardOrigin`] |
//! | request `Origin` not allowed by the list/predicate | `403`, [`DenyReason::OriginNotAllowed`], origin **not** echoed |
//! | preflight `Access-Control-Request-Method` not allowed (methods are case-sensitive per RFC 9110 section 9.1) | `403`, [`DenyReason::MethodNotAllowed`] |
//! | preflight `Access-Control-Request-Headers` entry not allowed (field names are case-insensitive per RFC 9110 section 5.1) | `403`, [`DenyReason::RequestHeadersNotAllowed`] |
//! | anything else | allowed; preflights are answered directly, actual requests are forwarded to the inner service with the sealed headers merged in |
//!
//! `Vary` values already set by the inner service are preserved; the policy's
//! own `Vary` entries are appended, never overwritten.
//!
//! # Example
//!
//! ```
//! use http::{Method, Request, Response, StatusCode, header::HeaderValue};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{Layer, ServiceExt, Service};
//! use tower_http::cors::{CorsLayer, DecisionLayer};
//! use std::convert::Infallible;
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Seal the configuration into an explicit, validated policy snapshot.
//! let policy = CorsLayer::new()
//!     .allow_origin(["https://example.com".parse::<HeaderValue>()?])
//!     .allow_methods([Method::GET])
//!     .allow_credentials(true)
//!     .seal()?;
//!
//! let mut service = DecisionLayer::new(policy).layer(tower::service_fn(handle));
//!
//! // Unknown origins are rejected without being echoed back and without
//! // executing the inner service.
//! let response = service
//!     .ready()
//!     .await?
//!     .call(
//!         Request::builder()
//!             .header(http::header::ORIGIN, "https://evil.example")
//!             .body(Full::default())?,
//!     )
//!     .await?;
//! assert_eq!(response.status(), StatusCode::FORBIDDEN);
//! # Ok(())
//! # }
//! ```

use http::{
    header::{self, HeaderName, HeaderValue},
    request::Parts as RequestParts,
    HeaderMap, Method, Request, Response, StatusCode,
};
use pin_project_lite::pin_project;
use std::{
    error::Error,
    fmt,
    future::Future,
    mem,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

use super::{
    allow_origin::AllowOriginFuture, AllowCredentials, AllowHeaders, AllowMethods, AllowOrigin,
    AllowPrivateNetwork, CorsLayer, ExposeHeaders, MaxAge, Vary,
};

/// A sealed, immutable CORS policy snapshot.
///
/// Created by sealing a [`CorsLayer`] via [`CorsLayer::seal`]. Sealing
/// validates the configuration (see [`InvalidCorsPolicy`]) and freezes it, so
/// preflight and actual requests are guaranteed to be evaluated against the
/// exact same policy.
///
/// Use [`CorsPolicy::decide`] to evaluate a single request explicitly, or
/// [`CorsPolicy::into_layer`]/[`DecisionLayer`] to apply the policy as a
/// middleware.
#[derive(Debug, Clone)]
#[must_use]
pub struct CorsPolicy {
    allow_credentials: AllowCredentials,
    allow_headers: AllowHeaders,
    allow_methods: AllowMethods,
    allow_origin: AllowOrigin,
    allow_private_network: AllowPrivateNetwork,
    expose_headers: ExposeHeaders,
    max_age: MaxAge,
    vary: Vary,
}

impl CorsLayer {
    /// Seal this configuration into an explicit, immutable [`CorsPolicy`]
    /// snapshot.
    ///
    /// Unlike [`CorsLayer`] itself, which defers validation to layer
    /// application / request time and panics on invalid combinations, sealing
    /// checks the `Access-Control-Allow-Credentials` combinations eagerly and
    /// reports them as a recoverable [`InvalidCorsPolicy`] error.
    ///
    /// The returned policy is shared unchanged by preflight and actual
    /// requests.
    pub fn seal(mut self) -> Result<CorsPolicy, InvalidCorsPolicy> {
        self.update_vary_header();
        CorsPolicy::seal(self)
    }
}

impl CorsPolicy {
    fn seal(layer: CorsLayer) -> Result<Self, InvalidCorsPolicy> {
        // The origin/credentials combination is checked first: a wildcard
        // origin must never be combined with credentialed access.
        if layer.allow_credentials.is_true() {
            if layer.allow_origin.is_wildcard() {
                return Err(InvalidCorsPolicy::CredentialsWithWildcardOrigin);
            }
            if layer.allow_methods.is_wildcard() {
                return Err(InvalidCorsPolicy::CredentialsWithWildcardAllowMethods);
            }
            if layer.allow_headers.is_wildcard() {
                return Err(InvalidCorsPolicy::CredentialsWithWildcardAllowHeaders);
            }
            if layer.expose_headers.is_wildcard() {
                return Err(InvalidCorsPolicy::CredentialsWithWildcardExposeHeaders);
            }
        }

        Ok(Self {
            allow_credentials: layer.allow_credentials,
            allow_headers: layer.allow_headers,
            allow_methods: layer.allow_methods,
            allow_origin: layer.allow_origin,
            allow_private_network: layer.allow_private_network,
            expose_headers: layer.expose_headers,
            max_age: layer.max_age,
            vary: layer.vary,
        })
    }

    /// Evaluate a single request against this sealed policy, producing an
    /// explicit [`Decision`] snapshot.
    ///
    /// Dynamic policies (predicates) are evaluated at most once per `decide`
    /// call.
    pub async fn decide(&self, parts: &RequestParts) -> Decision {
        let (mut prepared, allow_origin_future) = self.prepare(parts);
        let allow_origin = allow_origin_future.await;
        self.finish(&mut prepared, allow_origin)
    }

    /// Wrap this policy in a [`DecisionLayer`] middleware.
    pub fn into_layer(self) -> DecisionLayer {
        DecisionLayer::new(self)
    }

    /// Compute everything that can be computed synchronously for a request.
    ///
    /// Dynamic predicates are invoked at most once, here, and their results
    /// are carried through the returned future / [`PreparedDecision`].
    fn prepare(&self, parts: &RequestParts) -> (PreparedDecision, AllowOriginFuture) {
        let origin = parts.headers.get(header::ORIGIN);
        let input = RequestInput::from_parts(parts);

        // Headers shared by preflight and actual responses:
        // https://fetch.spec.whatwg.org/#http-responses
        let mut headers = HeaderMap::new();
        headers.extend(self.allow_credentials.to_header(origin, parts));
        let credentials_allowed = headers.contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS);
        headers.extend(self.allow_private_network.to_header(origin, parts));
        let vary = self.vary.to_header();

        if input.is_preflight {
            // These headers are applied only to preflight requests
            headers.extend(self.allow_methods.to_header(parts));
            headers.extend(self.allow_headers.to_header(parts));
            headers.extend(self.max_age.to_header(origin, parts));
        } else {
            // This header is applied only to non-preflight requests
            headers.extend(self.expose_headers.to_header(parts));
        }

        // Evaluated at most once per request, even for async predicates.
        let allow_origin_future = self.allow_origin.to_future(origin, parts);

        (
            PreparedDecision {
                input,
                headers,
                vary,
                credentials_allowed,
            },
            allow_origin_future,
        )
    }

    /// Combine the prepared state with the (possibly asynchronously) resolved
    /// allow-origin result into a final, explicit decision.
    fn finish(
        &self,
        prepared: &mut PreparedDecision,
        allow_origin: Option<(HeaderName, HeaderValue)>,
    ) -> Decision {
        let vary = prepared.vary.take();
        let deny = |reason: DenyReason| {
            // Denied responses carry the policy's `Vary` values (so caches
            // stay correct) but never echo the origin or any other
            // `Access-Control-Allow-*` header.
            let mut headers = HeaderMap::new();
            if let Some((name, value)) = vary.clone() {
                headers.append(name, value);
            }
            Decision::Deny(DenyDecision { reason, headers })
        };

        // The origin/credentials combination is checked first: a wildcard
        // origin must never be combined with credentialed access. Static
        // configurations are already rejected when sealing; this catches
        // predicate-based credentials at request time.
        if prepared.credentials_allowed && self.allow_origin.is_wildcard() {
            return deny(DenyReason::CredentialsWithWildcardOrigin);
        }

        // Unknown origins are rejected without being echoed back.
        if prepared.input.origin.is_some() && allow_origin.is_none() {
            return deny(DenyReason::OriginNotAllowed);
        }

        if prepared.input.is_preflight {
            // HTTP methods are case-sensitive tokens (RFC 9110 section 9.1).
            match &prepared.input.requested_method {
                Some(Ok(method)) if self.allow_methods.allows(method) => {}
                _ => return deny(DenyReason::MethodNotAllowed),
            }

            // HTTP field names are case-insensitive (RFC 9110 section 5.1).
            if let Some(requested_headers) = &prepared.input.requested_headers {
                let allowed = match requested_headers {
                    Ok(names) => names.iter().all(|name| self.allow_headers.allows(name)),
                    Err(()) => false,
                };
                if !allowed {
                    return deny(DenyReason::RequestHeadersNotAllowed);
                }
            }
        }

        let mut headers = mem::take(&mut prepared.headers);
        headers.extend(allow_origin);
        if let Some((name, value)) = vary {
            headers.append(name, value);
        }

        Decision::Allow(AllowDecision {
            preflight: prepared.input.is_preflight,
            headers,
        })
    }
}

/// Everything about a request that a decision depends on, captured once.
#[derive(Debug)]
struct RequestInput {
    origin: Option<HeaderValue>,
    is_preflight: bool,
    requested_method: Option<Result<Method, ()>>,
    requested_headers: Option<Result<Vec<HeaderName>, ()>>,
}

impl RequestInput {
    fn from_parts(parts: &RequestParts) -> Self {
        let origin = parts.headers.get(header::ORIGIN).cloned();
        let requested_method = parts
            .headers
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .map(|value| Method::from_bytes(value.as_bytes()).map_err(|_| ()));
        // A preflight is an `OPTIONS` request carrying
        // `Access-Control-Request-Method`; a plain `OPTIONS` is an actual
        // request and is forwarded to the inner service.
        let is_preflight = parts.method == Method::OPTIONS && requested_method.is_some();
        let requested_headers = if is_preflight {
            parts
                .headers
                .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
                .map(parse_header_list)
        } else {
            None
        };

        Self {
            origin,
            is_preflight,
            requested_method,
            requested_headers,
        }
    }
}

/// Parse a comma-separated list of HTTP field names.
///
/// Field names are case-insensitive, so they are normalized into
/// [`HeaderName`]s (which lowercase their input).
fn parse_header_list(value: &HeaderValue) -> Result<Vec<HeaderName>, ()> {
    value
        .as_bytes()
        .split(|b| *b == b',')
        .map(|token| HeaderName::from_bytes(trim(token)).map_err(|_| ()))
        .collect()
}

/// Trim optional whitespace (spaces and horizontal tabs) around a token.
pub(super) fn trim(mut bytes: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = bytes {
        if matches!(first, b' ' | b'\t') {
            bytes = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = bytes {
        if matches!(last, b' ' | b'\t') {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// The synchronously computed part of a decision.
#[derive(Debug)]
struct PreparedDecision {
    input: RequestInput,
    headers: HeaderMap,
    vary: Option<(HeaderName, HeaderValue)>,
    credentials_allowed: bool,
}

/// An explicit CORS decision snapshot for a single request.
///
/// Produced by [`CorsPolicy::decide`] and used internally by
/// [`DecisionService`]. The decision is final: preflight and actual responses
/// derived from it always reflect the same sealed policy.
#[derive(Debug)]
#[must_use]
pub enum Decision {
    /// The request is allowed by the sealed policy.
    Allow(AllowDecision),
    /// The request is denied; the inner service must not be executed.
    Deny(DenyDecision),
}

impl Decision {
    /// Whether the request is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow(_))
    }

    /// Whether the request is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, Decision::Deny(_))
    }
}

/// A snapshot of an allowed request: the kind of request and the exact,
/// sealed set of CORS response headers to apply.
#[derive(Debug)]
pub struct AllowDecision {
    preflight: bool,
    headers: HeaderMap,
}

impl AllowDecision {
    /// Whether this decision is for a CORS preflight request.
    pub fn is_preflight(&self) -> bool {
        self.preflight
    }

    /// The sealed CORS headers to apply to the response.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    fn into_headers(self) -> HeaderMap {
        self.headers
    }
}

/// A snapshot of a denied request: the reason and the (origin-echo-free)
/// headers to send back.
#[derive(Debug)]
pub struct DenyDecision {
    reason: DenyReason,
    headers: HeaderMap,
}

impl DenyDecision {
    /// Why the request was denied.
    pub fn reason(&self) -> DenyReason {
        self.reason
    }

    /// The headers to apply to the denial response. These contain the
    /// policy's `Vary` values only; the request origin is never echoed.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    fn into_headers(self) -> HeaderMap {
        self.headers
    }
}

/// Why a request was denied by a [`CorsPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DenyReason {
    /// Credentials would be allowed together with a wildcard (`*`) origin,
    /// which must never be permitted.
    CredentialsWithWildcardOrigin,
    /// The request origin is not allowed; it is not echoed back.
    OriginNotAllowed,
    /// The preflight `Access-Control-Request-Method` is not allowed.
    ///
    /// Methods are compared case-sensitively, per the HTTP specification.
    MethodNotAllowed,
    /// A preflight `Access-Control-Request-Headers` entry is not allowed.
    ///
    /// Header names are compared case-insensitively, per the HTTP
    /// specification.
    RequestHeadersNotAllowed,
}

impl fmt::Display for DenyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            DenyReason::CredentialsWithWildcardOrigin => {
                "cannot combine allowed credentials with a wildcard origin"
            }
            DenyReason::OriginNotAllowed => "request origin is not allowed",
            DenyReason::MethodNotAllowed => "request method is not allowed",
            DenyReason::RequestHeadersNotAllowed => "request headers are not allowed",
        };
        f.write_str(message)
    }
}

impl Error for DenyReason {}

/// Why a [`CorsLayer`] could not be sealed into a [`CorsPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvalidCorsPolicy {
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Allow-Origin: *`.
    CredentialsWithWildcardOrigin,
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Allow-Methods: *`.
    CredentialsWithWildcardAllowMethods,
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Allow-Headers: *`.
    CredentialsWithWildcardAllowHeaders,
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Expose-Headers: *`.
    CredentialsWithWildcardExposeHeaders,
}

impl fmt::Display for InvalidCorsPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let header = match self {
            InvalidCorsPolicy::CredentialsWithWildcardOrigin => "Access-Control-Allow-Origin: *",
            InvalidCorsPolicy::CredentialsWithWildcardAllowMethods => {
                "Access-Control-Allow-Methods: *"
            }
            InvalidCorsPolicy::CredentialsWithWildcardAllowHeaders => {
                "Access-Control-Allow-Headers: *"
            }
            InvalidCorsPolicy::CredentialsWithWildcardExposeHeaders => {
                "Access-Control-Expose-Headers: *"
            }
        };
        write!(
            f,
            "invalid CORS configuration: cannot combine \
             `Access-Control-Allow-Credentials: true` with `{header}`"
        )
    }
}

impl Error for InvalidCorsPolicy {}

/// Layer that applies a sealed [`CorsPolicy`] to preflight and actual
/// requests alike.
///
/// Unlike [`CorsLayer`], denied requests are answered with `403 Forbidden`
/// and never reach the inner service.
///
/// See the [module docs](self) for the policy table and an example.
#[derive(Debug, Clone)]
#[must_use]
pub struct DecisionLayer {
    policy: Arc<CorsPolicy>,
}

impl DecisionLayer {
    /// Create a `DecisionLayer` from a sealed policy.
    pub fn new(policy: CorsPolicy) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }

    /// Seal a [`CorsLayer`] configuration and create a `DecisionLayer` from
    /// the resulting policy.
    pub fn from_layer(layer: CorsLayer) -> Result<Self, InvalidCorsPolicy> {
        layer.seal().map(Self::new)
    }

    /// The sealed policy shared by this layer.
    pub fn policy(&self) -> &CorsPolicy {
        &self.policy
    }
}

impl<S> Layer<S> for DecisionLayer {
    type Service = DecisionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DecisionService {
            inner,
            policy: Arc::clone(&self.policy),
        }
    }
}

/// Middleware which applies a sealed [`CorsPolicy`], producing explicit
/// per-request [`Decision`] snapshots.
///
/// See the [module docs](self) for more details.
#[derive(Debug, Clone)]
#[must_use]
pub struct DecisionService<S> {
    inner: S,
    policy: Arc<CorsPolicy>,
}

impl<S> DecisionService<S> {
    /// Create a new `DecisionService` from an inner service and a sealed
    /// policy.
    pub fn new(inner: S, policy: CorsPolicy) -> Self {
        Self {
            inner,
            policy: Arc::new(policy),
        }
    }

    define_inner_service_accessors!();

    /// The sealed policy applied by this service.
    pub fn policy(&self) -> &CorsPolicy {
        &self.policy
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for DecisionService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ResBody: Default,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = DecisionFuture<S::Future, S, ReqBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();

        // Evaluate the request exactly once. Dynamic policies (predicates)
        // run here and only here; the sealed decision below is reused for
        // the whole request/response cycle.
        let (prepared, allow_origin_future) = self.policy.prepare(&parts);

        let req = Request::from_parts(parts, body);

        // Move a ready clone of the inner service into the future so the
        // decision can be resolved before (maybe) executing it. Denied
        // requests never touch it.
        let clone = self.inner.clone();
        let inner = mem::replace(&mut self.inner, clone);

        DecisionFuture {
            policy: Arc::clone(&self.policy),
            state: State::Deciding {
                allow_origin_future,
                prepared,
                inner: Some(inner),
                req: Some(req),
            },
        }
    }
}

pin_project! {
    /// Response future for [`DecisionService`].
    pub struct DecisionFuture<F, S, ReqBody> {
        policy: Arc<CorsPolicy>,
        #[pin]
        state: State<F, S, ReqBody>,
    }
}

pin_project! {
    #[project = StateProj]
    enum State<F, S, ReqBody> {
        Deciding {
            #[pin]
            allow_origin_future: AllowOriginFuture,
            prepared: PreparedDecision,
            inner: Option<S>,
            req: Option<Request<ReqBody>>,
        },
        Inner {
            #[pin]
            future: F,
            headers: HeaderMap,
        },
    }
}

impl<F, S, ReqBody, ResBody> Future for DecisionFuture<F, S, ReqBody>
where
    F: Future<Output = Result<Response<ResBody>, S::Error>>,
    S: Service<Request<ReqBody>, Response = Response<ResBody>, Future = F>,
    ResBody: Default,
{
    type Output = Result<Response<ResBody>, S::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            match this.state.as_mut().project() {
                StateProj::Deciding {
                    allow_origin_future,
                    prepared,
                    inner,
                    req,
                } => {
                    let allow_origin = ready!(allow_origin_future.poll(cx));
                    let decision = this.policy.finish(prepared, allow_origin);

                    match decision {
                        Decision::Allow(allow) if allow.is_preflight() => {
                            // Preflights are answered directly from the sealed
                            // decision; the inner service is not executed.
                            let mut response = Response::new(ResBody::default());
                            *response.headers_mut() = allow.into_headers();
                            return Poll::Ready(Ok(response));
                        }
                        Decision::Allow(allow) => {
                            let mut inner = inner.take().expect("future polled after completion");
                            let req = req.take().expect("future polled after completion");
                            let future = inner.call(req);
                            this.state.as_mut().set(State::Inner {
                                future,
                                headers: allow.into_headers(),
                            });
                        }
                        Decision::Deny(deny) => {
                            // Denied requests are rejected here; the inner
                            // service is dropped without being executed.
                            let mut response = Response::new(ResBody::default());
                            *response.status_mut() = StatusCode::FORBIDDEN;
                            *response.headers_mut() = deny.into_headers();
                            return Poll::Ready(Ok(response));
                        }
                    }
                }
                StateProj::Inner { future, headers } => {
                    let mut response = ready!(future.poll(cx))?;
                    let response_headers = response.headers_mut();

                    // vary header can have multiple values, don't overwrite
                    // previously-set value(s).
                    if let Some(vary) = headers.remove(header::VARY) {
                        response_headers.append(header::VARY, vary);
                    }
                    // extend will overwrite previous headers of remaining names
                    response_headers.extend(headers.drain());

                    return Poll::Ready(Ok(response));
                }
            }
        }
    }
}
