//! An explicit, sealed CORS policy with per-request decision snapshots.
//!
//! Unlike [`CorsLayer`](super::CorsLayer), which applies each CORS rule
//! independently, a [`CorsPolicy`] is *sealed* once (via
//! [`CorsPolicyBuilder::seal`]) and then maps every request to exactly one
//! explicit [`Decision`]. Both preflight (`OPTIONS`) and actual requests are
//! answered from the same frozen [`DecisionSnapshot`], computed exactly once
//! per request — dynamic origin predicates included.
//!
//! # Policy decision table
//!
//! Decisions are evaluated in a fixed order. The origin/credentials
//! combination is validated first (at seal time, before any request is
//! served), then the request origin, then preflight method and headers:
//!
//! | # | Request                                   | Check (in order)                                         | Decision                      | Inner service |
//! |---|-------------------------------------------|----------------------------------------------------------|-------------------------------|---------------|
//! | 1 | `allow_credentials` + any wildcard (`*`)  | rejected when the policy is sealed                       | [`PolicyError`]               | n/a           |
//! | 2 | no `Origin` header                        | not a CORS request                                       | [`Decision::PassThrough`]     | called        |
//! | 3 | `Origin` not in the allowed set           | origin checked first, unknown origins are never echoed   | [`Decision::Deny`]`(OriginNotAllowed)` | never |
//! | 4 | preflight, malformed method/header tokens | parse `Access-Control-Request-*`                         | [`Decision::Deny`]`(MalformedPreflight)` | never |
//! | 5 | preflight, method not allowed             | methods compared case-sensitively (RFC 9110, sec. 9.1)   | [`Decision::Deny`]`(MethodNotAllowed)` | never |
//! | 6 | preflight, header not allowed             | header names compared case-insensitively (RFC 9110, sec. 5.1) | [`Decision::Deny`]`(HeaderNotAllowed)` | never |
//! | 7 | preflight, all checks pass                | snapshot frozen once, reused for the whole response      | [`Decision::Allow`]           | not called    |
//! | 8 | actual request, origin allowed            | same sealed policy as the preflight                      | [`Decision::Allow`]           | called        |
//!
//! Denied requests are answered with `403 Forbidden`, carry **no** CORS
//! headers, never echo the offending origin, and never execute the inner
//! service. `Vary` values set by the inner service are preserved and merged
//! (case-insensitively deduplicated) with the policy's own `Vary` entries.
//!
//! # Example
//!
//! ```
//! use http::{Method, Request, Response, header};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use tower::{ServiceBuilder, ServiceExt, Service};
//! use tower_http::cors::policy::CorsPolicy;
//! use std::convert::Infallible;
//!
//! async fn handle(request: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//!     Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let policy = CorsPolicy::builder()
//!     .allow_credentials(true)
//!     .allow_origins(["https://app.example".parse().unwrap()])
//!     .allow_methods([Method::GET, Method::POST])
//!     .allow_headers([header::CONTENT_TYPE])
//!     .seal()?;
//!
//! let mut service = ServiceBuilder::new()
//!     .layer(policy.layer())
//!     .service_fn(handle);
//!
//! let request = Request::builder()
//!     .header(header::ORIGIN, "https://app.example")
//!     .body(Full::default())
//!     .unwrap();
//!
//! let response = service.ready().await?.call(request).await?;
//!
//! assert_eq!(
//!     response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
//!     "https://app.example",
//! );
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
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
    time::Duration,
};
use tower_layer::Layer;
use tower_service::Service;

use super::{separated_by_commas, WILDCARD};

#[allow(clippy::declare_interior_mutable_const)]
const TRUE: HeaderValue = HeaderValue::from_static("true");

/// A sealed, immutable CORS policy.
///
/// Built with [`CorsPolicy::builder`] and frozen by
/// [`CorsPolicyBuilder::seal`]. Sealing validates the origin/credentials
/// combination up front, so an invalid policy can never illegally allow a
/// request at runtime.
#[derive(Debug, Clone)]
pub struct CorsPolicy {
    inner: Arc<PolicyInner>,
}

#[derive(Debug)]
struct PolicyInner {
    allow_credentials: bool,
    origins: OriginRule,
    methods: MethodRule,
    methods_value: Option<HeaderValue>,
    headers: HeaderRule,
    headers_value: Option<HeaderValue>,
    expose_headers_value: Option<HeaderValue>,
    max_age_value: Option<HeaderValue>,
    vary: Vec<HeaderValue>,
}

#[derive(Clone)]
enum OriginRule {
    Any,
    List(Vec<HeaderValue>),
    Predicate(
        Arc<dyn for<'a> Fn(&'a HeaderValue, &'a RequestParts) -> bool + Send + Sync + 'static>,
    ),
}

impl fmt::Debug for OriginRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OriginRule::Any => f.write_str("Any"),
            OriginRule::List(list) => f.debug_tuple("List").field(list).finish(),
            OriginRule::Predicate(_) => f.write_str("Predicate"),
        }
    }
}

#[derive(Clone, Debug)]
enum MethodRule {
    Any,
    List(Vec<Method>),
}

#[derive(Clone, Debug)]
enum HeaderRule {
    Any,
    List(Vec<HeaderName>),
}

/// Errors returned when sealing an invalid [`CorsPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Allow-Origin: *`.
    CredentialsWithWildcardOrigin,
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Allow-Methods: *`.
    CredentialsWithWildcardMethods,
    /// `Access-Control-Allow-Credentials: true` cannot be combined with
    /// `Access-Control-Allow-Headers: *`.
    CredentialsWithWildcardHeaders,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::CredentialsWithWildcardOrigin => f.write_str(
                "invalid CORS policy: cannot combine `Access-Control-Allow-Credentials: true` \
                 with `Access-Control-Allow-Origin: *`",
            ),
            PolicyError::CredentialsWithWildcardMethods => f.write_str(
                "invalid CORS policy: cannot combine `Access-Control-Allow-Credentials: true` \
                 with `Access-Control-Allow-Methods: *`",
            ),
            PolicyError::CredentialsWithWildcardHeaders => f.write_str(
                "invalid CORS policy: cannot combine `Access-Control-Allow-Credentials: true` \
                 with `Access-Control-Allow-Headers: *`",
            ),
        }
    }
}

impl std::error::Error for PolicyError {}

/// Why a request was denied by a [`CorsPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// The request origin is not in the policy's allowed set. The origin is
    /// never echoed back in the response.
    OriginNotAllowed,
    /// The preflight `Access-Control-Request-Method` is not allowed.
    /// Methods are matched case-sensitively, per RFC 9110, sec. 9.1.
    MethodNotAllowed,
    /// A preflight `Access-Control-Request-Headers` entry is not allowed.
    /// Header names are matched case-insensitively, per RFC 9110, sec. 5.1.
    HeaderNotAllowed,
    /// The preflight `Access-Control-Request-*` headers could not be parsed.
    MalformedPreflight,
}

/// The explicit outcome of evaluating a sealed [`CorsPolicy`] against one
/// request.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// The request is allowed. The attached snapshot contains every CORS
    /// header value the response will carry, frozen at decision time.
    Allow(DecisionSnapshot),
    /// The request is denied. The inner service is never executed and no
    /// CORS headers (and no origin echo) are emitted.
    Deny(DenyReason),
    /// The request carries no `Origin` header and is not a CORS request.
    /// It is passed to the inner service without any CORS headers.
    PassThrough,
}

/// A frozen, per-request snapshot of a [`CorsPolicy`] decision.
///
/// The same snapshot type backs both preflight and actual responses, so a
/// preflight and the request it authorizes always observe the identical
/// sealed policy.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionSnapshot {
    is_preflight: bool,
    origin: HeaderValue,
    allow_credentials: bool,
    allow_methods: Option<HeaderValue>,
    allow_headers: Option<HeaderValue>,
    expose_headers: Option<HeaderValue>,
    max_age: Option<HeaderValue>,
    vary: Vec<HeaderValue>,
}

impl DecisionSnapshot {
    /// Whether this snapshot authorizes a preflight (`OPTIONS`) request.
    pub fn is_preflight(&self) -> bool {
        self.is_preflight
    }

    /// The resolved `Access-Control-Allow-Origin` value. This is always a
    /// value from the sealed policy (or `*`); unknown origins are denied
    /// instead of echoed.
    pub fn allow_origin(&self) -> &HeaderValue {
        &self.origin
    }

    /// Whether `Access-Control-Allow-Credentials: true` will be sent.
    pub fn allow_credentials(&self) -> bool {
        self.allow_credentials
    }

    /// The frozen `Access-Control-Allow-Methods` value (preflight only).
    pub fn allow_methods(&self) -> Option<&HeaderValue> {
        self.allow_methods.as_ref()
    }

    /// The frozen `Access-Control-Allow-Headers` value (preflight only).
    pub fn allow_headers(&self) -> Option<&HeaderValue> {
        self.allow_headers.as_ref()
    }

    /// The frozen `Access-Control-Expose-Headers` value (actual requests only).
    pub fn expose_headers(&self) -> Option<&HeaderValue> {
        self.expose_headers.as_ref()
    }

    /// The frozen `Access-Control-Max-Age` value (preflight only).
    pub fn max_age(&self) -> Option<&HeaderValue> {
        self.max_age.as_ref()
    }

    /// The frozen `Vary` entries, merged into the response without dropping
    /// values already set by the inner service.
    pub fn vary(&self) -> &[HeaderValue] {
        &self.vary
    }

    fn apply_preflight_headers(&self, headers: &mut HeaderMap) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, self.origin.clone());
        if self.allow_credentials {
            headers.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, TRUE);
        }
        if let Some(allow_methods) = &self.allow_methods {
            headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, allow_methods.clone());
        }
        if let Some(allow_headers) = &self.allow_headers {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, allow_headers.clone());
        }
        if let Some(max_age) = &self.max_age {
            headers.insert(header::ACCESS_CONTROL_MAX_AGE, max_age.clone());
        }
        merge_vary(headers, &self.vary);
    }

    fn apply_actual_headers(&self, headers: &mut HeaderMap) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, self.origin.clone());
        if self.allow_credentials {
            headers.insert(header::ACCESS_CONTROL_ALLOW_CREDENTIALS, TRUE);
        }
        if let Some(expose_headers) = &self.expose_headers {
            headers.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                expose_headers.clone(),
            );
        }
        merge_vary(headers, &self.vary);
    }
}

/// Builder for a [`CorsPolicy`]. Obtained from [`CorsPolicy::builder`].
#[derive(Debug, Clone, Default)]
pub struct CorsPolicyBuilder {
    allow_credentials: bool,
    origins: Option<OriginRule>,
    methods: Option<MethodRule>,
    headers: Option<HeaderRule>,
    expose_headers: Option<Vec<HeaderName>>,
    max_age: Option<Duration>,
    vary: Option<Vec<HeaderName>>,
}

impl CorsPolicy {
    /// Start building a new policy.
    ///
    /// By default no origins, methods or headers are allowed: every CORS
    /// request is denied until the policy explicitly allows it.
    pub fn builder() -> CorsPolicyBuilder {
        CorsPolicyBuilder::default()
    }

    /// Returns a [`Layer`] that applies this sealed policy to a service.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer(&self) -> PolicyLayer {
        PolicyLayer::new(self.clone())
    }

    /// Evaluate the sealed policy against one request, producing an explicit
    /// [`Decision`].
    ///
    /// This is the only place dynamic origin predicates run; the middleware
    /// calls it exactly once per request and freezes the result into a
    /// [`DecisionSnapshot`].
    pub fn decide(&self, parts: &RequestParts) -> Decision {
        let inner = &*self.inner;

        let origin = match parts.headers.get(header::ORIGIN) {
            Some(origin) => origin,
            None => return Decision::PassThrough,
        };

        let is_preflight = parts.method == Method::OPTIONS
            && parts
                .headers
                .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);

        // 1. Origin first. Unknown origins are rejected and never echoed;
        //    the emitted value always comes from the sealed policy. The
        //    credentials/wildcard combination was already rejected at seal
        //    time, so a wildcard here can never illegally allow credentials.
        let resolved_origin = match &inner.origins {
            OriginRule::Any => WILDCARD,
            OriginRule::List(list) => match list.iter().find(|known| *known == origin) {
                Some(known) => known.clone(),
                None => return Decision::Deny(DenyReason::OriginNotAllowed),
            },
            OriginRule::Predicate(predicate) => {
                if predicate(origin, parts) {
                    origin.clone()
                } else {
                    return Decision::Deny(DenyReason::OriginNotAllowed);
                }
            }
        };

        let mut allow_methods = None;
        let mut allow_headers = None;
        let mut max_age = None;

        if is_preflight {
            // 2. Method. Methods are case-sensitive tokens (RFC 9110,
            //    sec. 9.1), so the requested method must match exactly.
            if let MethodRule::List(list) = &inner.methods {
                let requested = parts
                    .headers
                    .get(header::ACCESS_CONTROL_REQUEST_METHOD)
                    .expect("preflight requests carry Access-Control-Request-Method");
                let requested = match Method::from_bytes(requested.as_bytes()) {
                    Ok(method) => method,
                    Err(_) => return Decision::Deny(DenyReason::MalformedPreflight),
                };
                if !list.contains(&requested) {
                    return Decision::Deny(DenyReason::MethodNotAllowed);
                }
            }
            allow_methods = inner.methods_value.clone();

            // 3. Headers. Field names are case-insensitive (RFC 9110,
            //    sec. 5.1); `HeaderName` normalizes to lowercase before
            //    comparison.
            if let HeaderRule::List(list) = &inner.headers {
                if let Some(requested) = parts.headers.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
                    for name in requested.as_bytes().split(|b| *b == b',') {
                        let name = trim_ascii(name);
                        if name.is_empty() {
                            continue;
                        }
                        let name = match HeaderName::from_bytes(name) {
                            Ok(name) => name,
                            Err(_) => return Decision::Deny(DenyReason::MalformedPreflight),
                        };
                        if !list.contains(&name) {
                            return Decision::Deny(DenyReason::HeaderNotAllowed);
                        }
                    }
                }
            }
            allow_headers = inner.headers_value.clone();

            max_age = inner.max_age_value.clone();
        }

        Decision::Allow(DecisionSnapshot {
            is_preflight,
            origin: resolved_origin,
            allow_credentials: inner.allow_credentials,
            allow_methods,
            allow_headers,
            expose_headers: if is_preflight {
                None
            } else {
                inner.expose_headers_value.clone()
            },
            max_age,
            vary: inner.vary.clone(),
        })
    }
}

impl CorsPolicyBuilder {
    /// Set whether `Access-Control-Allow-Credentials: true` is sent.
    ///
    /// Note that credentials cannot be combined with any wildcard (`*`)
    /// rule; such policies are rejected by [`seal`](Self::seal).
    pub fn allow_credentials(mut self, allow_credentials: bool) -> Self {
        self.allow_credentials = allow_credentials;
        self
    }

    /// Allow any origin by sending a wildcard (`*`).
    pub fn allow_any_origin(mut self) -> Self {
        self.origins = Some(OriginRule::Any);
        self
    }

    /// Allow the exact origins in the list. Origins not in the list are
    /// denied and never echoed back.
    ///
    /// # Panics
    ///
    /// If the iterator contains a wildcard (`*`); use
    /// [`allow_any_origin`](Self::allow_any_origin) instead.
    #[allow(clippy::borrow_interior_mutable_const)]
    pub fn allow_origins<I>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = HeaderValue>,
    {
        let origins = origins.into_iter().collect::<Vec<_>>();
        if origins.contains(&WILDCARD) {
            panic!(
                "Wildcard origin (`*`) cannot be passed to `allow_origins`. \
                 Use `allow_any_origin` instead"
            );
        }
        self.origins = Some(OriginRule::List(origins));
        self
    }

    /// Allow origins accepted by a predicate.
    ///
    /// The predicate runs exactly once per request, inside
    /// [`CorsPolicy::decide`].
    pub fn allow_origin_predicate<F>(mut self, f: F) -> Self
    where
        F: Fn(&HeaderValue, &RequestParts) -> bool + Send + Sync + 'static,
    {
        self.origins = Some(OriginRule::Predicate(Arc::new(f)));
        self
    }

    /// Allow any method by sending a wildcard (`*`).
    pub fn allow_any_methods(mut self) -> Self {
        self.methods = Some(MethodRule::Any);
        self
    }

    /// Allow the given methods. Preflight methods are matched
    /// case-sensitively, per RFC 9110, sec. 9.1.
    pub fn allow_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = Method>,
    {
        self.methods = Some(MethodRule::List(methods.into_iter().collect()));
        self
    }

    /// Allow any request headers by sending a wildcard (`*`).
    pub fn allow_any_headers(mut self) -> Self {
        self.headers = Some(HeaderRule::Any);
        self
    }

    /// Allow the given request headers. Header names are matched
    /// case-insensitively, per RFC 9110, sec. 5.1.
    pub fn allow_headers<I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        self.headers = Some(HeaderRule::List(headers.into_iter().collect()));
        self
    }

    /// Set the `Access-Control-Expose-Headers` value for actual requests.
    pub fn expose_headers<I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        self.expose_headers = Some(headers.into_iter().collect());
        self
    }

    /// Set the `Access-Control-Max-Age` value for preflight responses.
    pub fn max_age(mut self, max_age: Duration) -> Self {
        self.max_age = Some(max_age);
        self
    }

    /// Pin the `Vary` entries explicitly instead of deriving them.
    pub fn vary<I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        self.vary = Some(headers.into_iter().collect());
        self
    }

    /// Validate and freeze the policy.
    ///
    /// The origin/credentials combination is checked first: sealing fails if
    /// `allow_credentials(true)` is combined with any wildcard (`*`) origin,
    /// method or header rule, so an invalid policy can never illegally allow
    /// a request at runtime.
    pub fn seal(self) -> Result<CorsPolicy, PolicyError> {
        if self.allow_credentials {
            if matches!(self.origins, Some(OriginRule::Any)) {
                return Err(PolicyError::CredentialsWithWildcardOrigin);
            }
            if matches!(self.methods, Some(MethodRule::Any)) {
                return Err(PolicyError::CredentialsWithWildcardMethods);
            }
            if matches!(self.headers, Some(HeaderRule::Any)) {
                return Err(PolicyError::CredentialsWithWildcardHeaders);
            }
        }

        let origins = self.origins.unwrap_or(OriginRule::List(Vec::new()));
        let methods = self.methods.unwrap_or(MethodRule::List(Vec::new()));
        let headers = self.headers.unwrap_or(HeaderRule::List(Vec::new()));

        let methods_value = match &methods {
            MethodRule::Any => Some(WILDCARD),
            MethodRule::List(list) => separated_by_commas(
                list.iter()
                    .map(|m| HeaderValue::from_str(m.as_str()).unwrap()),
            ),
        };
        let headers_value = match &headers {
            HeaderRule::Any => Some(WILDCARD),
            HeaderRule::List(list) => {
                separated_by_commas(list.iter().cloned().map(HeaderValue::from))
            }
        };
        let expose_headers_value = self
            .expose_headers
            .and_then(|list| separated_by_commas(list.into_iter().map(HeaderValue::from)));
        let max_age_value = self
            .max_age
            .map(|d| HeaderValue::from_str(&d.as_secs().to_string()).unwrap());

        let vary = match self.vary {
            Some(list) => list.into_iter().map(HeaderValue::from).collect(),
            None => {
                // Derive Vary from the request-dependent rules.
                let mut vary = Vec::new();
                if !matches!(origins, OriginRule::Any) {
                    vary.push(HeaderValue::from_static("origin"));
                }
                if matches!(methods, MethodRule::List(_)) {
                    vary.push(HeaderValue::from_static("access-control-request-method"));
                }
                if matches!(headers, HeaderRule::List(_)) {
                    vary.push(HeaderValue::from_static("access-control-request-headers"));
                }
                vary
            }
        };

        Ok(CorsPolicy {
            inner: Arc::new(PolicyInner {
                allow_credentials: self.allow_credentials,
                origins,
                methods,
                methods_value,
                headers,
                headers_value,
                expose_headers_value,
                max_age_value,
                vary,
            }),
        })
    }
}

/// Merge `entries` into the response's `Vary` header without dropping any
/// values already set by the inner service. Entries are deduplicated
/// case-insensitively (field names are case-insensitive per RFC 9110,
/// sec. 5.1).
fn merge_vary(headers: &mut HeaderMap, entries: &[HeaderValue]) {
    if entries.is_empty() {
        return;
    }

    let mut tokens: Vec<HeaderValue> = Vec::new();
    for value in headers.get_all(header::VARY).iter() {
        for part in value.as_bytes().split(|b| *b == b',') {
            let part = trim_ascii(part);
            if part.is_empty() {
                continue;
            }
            if part == b"*" {
                // `Vary: *` already disables caching; nothing to add.
                return;
            }
            if let Ok(value) = HeaderValue::from_bytes(part) {
                tokens.push(value);
            }
        }
    }

    for entry in entries {
        let known = tokens
            .iter()
            .any(|token| token.as_bytes().eq_ignore_ascii_case(entry.as_bytes()));
        if !known {
            tokens.push(entry.clone());
        }
    }

    if let Some(joined) = separated_by_commas(tokens.into_iter()) {
        headers.insert(header::VARY, joined);
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = bytes.split_last() {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// Layer that applies a sealed [`CorsPolicy`].
///
/// See the [module docs](self) for the decision table.
#[derive(Debug, Clone)]
#[must_use]
pub struct PolicyLayer {
    policy: CorsPolicy,
}

impl PolicyLayer {
    /// Create a new `PolicyLayer` from a sealed policy.
    pub fn new(policy: CorsPolicy) -> Self {
        Self { policy }
    }
}

impl<S> Layer<S> for PolicyLayer {
    type Service = PolicyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PolicyService {
            inner,
            policy: self.policy.clone(),
        }
    }
}

/// Middleware that applies a sealed [`CorsPolicy`].
///
/// See the [module docs](self) for the decision table.
#[derive(Debug, Clone)]
#[must_use]
pub struct PolicyService<S> {
    inner: S,
    policy: CorsPolicy,
}

impl<S> PolicyService<S> {
    /// Create a new `PolicyService` from a sealed policy.
    pub fn new(inner: S, policy: CorsPolicy) -> Self {
        Self { inner, policy }
    }

    define_inner_service_accessors!();
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for PolicyService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = PolicyResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let (parts, body) = req.into_parts();

        // The decision — including any dynamic origin predicate — is
        // computed exactly once per request and frozen into a snapshot that
        // backs the whole response.
        match self.policy.decide(&parts) {
            Decision::PassThrough => PolicyResponseFuture {
                kind: PolicyKind::Inner {
                    future: self.inner.call(Request::from_parts(parts, body)),
                    snapshot: None,
                },
            },
            Decision::Deny(_reason) => {
                // Denied requests never reach the inner service and never
                // echo the offending origin.
                let mut response = Response::new(ResBody::default());
                *response.status_mut() = StatusCode::FORBIDDEN;
                PolicyResponseFuture {
                    kind: PolicyKind::Ready {
                        response: Some(response),
                    },
                }
            }
            Decision::Allow(snapshot) if snapshot.is_preflight() => {
                let mut response = Response::new(ResBody::default());
                snapshot.apply_preflight_headers(response.headers_mut());
                PolicyResponseFuture {
                    kind: PolicyKind::Ready {
                        response: Some(response),
                    },
                }
            }
            Decision::Allow(snapshot) => PolicyResponseFuture {
                kind: PolicyKind::Inner {
                    future: self.inner.call(Request::from_parts(parts, body)),
                    snapshot: Some(snapshot),
                },
            },
        }
    }
}

pin_project! {
    /// Response future for [`PolicyService`].
    pub struct PolicyResponseFuture<F, B> {
        #[pin]
        kind: PolicyKind<F, B>,
    }
}

pin_project! {
    #[project = PolicyKindProj]
    enum PolicyKind<F, B> {
        Inner {
            #[pin]
            future: F,
            snapshot: Option<DecisionSnapshot>,
        },
        Ready {
            response: Option<Response<B>>,
        },
    }
}

impl<F, B, E> Future for PolicyResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = Result<Response<B>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            PolicyKindProj::Inner { future, snapshot } => {
                let mut response = ready!(future.poll(cx))?;
                if let Some(snapshot) = snapshot.take() {
                    snapshot.apply_actual_headers(response.headers_mut());
                }
                Poll::Ready(Ok(response))
            }
            PolicyKindProj::Ready { response } => Poll::Ready(Ok(response
                .take()
                .expect("PolicyResponseFuture polled after completion"))),
        }
    }
}
