//! Middlewares that mark headers as [sensitive].
//!
//! [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
//!
//! # Example
//!
//! ```
//! use tower_http::sensitive_headers::SetSensitiveHeadersLayer;
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use http::{Request, Response, header::AUTHORIZATION};
//! use http_body_util::Full;
//! use bytes::Bytes;
//! use std::{iter::once, convert::Infallible};
//!
//! async fn handle(req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//!     // ...
//!     # Ok(Response::new(Full::default()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut service = ServiceBuilder::new()
//!     // Mark the `Authorization` header as sensitive so it doesn't show in logs
//!     //
//!     // `SetSensitiveHeadersLayer` will mark the header as sensitive on both the
//!     // request and response.
//!     //
//!     // The middleware is constructed from an iterator of headers to easily mark
//!     // multiple headers at once.
//!     .layer(SetSensitiveHeadersLayer::new(once(AUTHORIZATION)))
//!     .service(service_fn(handle));
//!
//! // Call the service.
//! let response = service
//!     .ready()
//!     .await?
//!     .call(Request::new(Full::default()))
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! Its important to think about the order in which requests and responses arrive at your
//! middleware. For example to hide headers both on requests and responses when using
//! [`TraceLayer`] you have to apply [`SetSensitiveRequestHeadersLayer`] before [`TraceLayer`]
//! and [`SetSensitiveResponseHeadersLayer`] afterwards.
//!
//! ```
//! use tower_http::{
//!     trace::TraceLayer,
//!     sensitive_headers::{
//!         SetSensitiveRequestHeadersLayer,
//!         SetSensitiveResponseHeadersLayer,
//!     },
//! };
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//! use http::header;
//! use std::sync::Arc;
//! # use http::{Request, Response};
//! # use bytes::Bytes;
//! # use http_body_util::Full;
//! # use std::convert::Infallible;
//! # async fn handle(req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>, Infallible> {
//! #     Ok(Response::new(Full::default()))
//! # }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let headers: Arc<[_]> = Arc::new([
//!     header::AUTHORIZATION,
//!     header::PROXY_AUTHORIZATION,
//!     header::COOKIE,
//!     header::SET_COOKIE,
//! ]);
//!
//! let service = ServiceBuilder::new()
//!     .layer(SetSensitiveRequestHeadersLayer::from_shared(Arc::clone(&headers)))
//!     .layer(TraceLayer::new_for_http())
//!     .layer(SetSensitiveResponseHeadersLayer::from_shared(headers))
//!     .service_fn(handle);
//! # Ok(())
//! # }
//! ```
//!
//! [`TraceLayer`]: crate::trace::TraceLayer

use http::{header::HeaderName, Request, Response};
use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Mark headers as [sensitive] on both requests and responses.
///
/// Produces [`SetSensitiveHeaders`] services.
///
/// See the [module docs](crate::sensitive_headers) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveHeadersLayer {
    headers: Arc<[HeaderName]>,
}

impl SetSensitiveHeadersLayer {
    /// Create a new [`SetSensitiveHeadersLayer`].
    pub fn new<I>(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        let headers = headers.into_iter().collect::<Vec<_>>();
        Self::from_shared(headers.into())
    }

    /// Create a new [`SetSensitiveHeadersLayer`] from a shared slice of headers.
    pub fn from_shared(headers: Arc<[HeaderName]>) -> Self {
        Self { headers }
    }
}

impl<S> Layer<S> for SetSensitiveHeadersLayer {
    type Service = SetSensitiveHeaders<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveRequestHeaders::from_shared(
            SetSensitiveResponseHeaders::from_shared(inner, self.headers.clone()),
            self.headers.clone(),
        )
    }
}

/// Mark headers as [sensitive] on both requests and responses.
///
/// See the [module docs](crate::sensitive_headers) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
pub type SetSensitiveHeaders<S> = SetSensitiveRequestHeaders<SetSensitiveResponseHeaders<S>>;

/// Mark request headers as [sensitive].
///
/// Produces [`SetSensitiveRequestHeaders`] services.
///
/// See the [module docs](crate::sensitive_headers) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveRequestHeadersLayer {
    headers: Arc<[HeaderName]>,
}

impl SetSensitiveRequestHeadersLayer {
    /// Create a new [`SetSensitiveRequestHeadersLayer`].
    pub fn new<I>(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        let headers = headers.into_iter().collect::<Vec<_>>();
        Self::from_shared(headers.into())
    }

    /// Create a new [`SetSensitiveRequestHeadersLayer`] from a shared slice of headers.
    pub fn from_shared(headers: Arc<[HeaderName]>) -> Self {
        Self { headers }
    }
}

impl<S> Layer<S> for SetSensitiveRequestHeadersLayer {
    type Service = SetSensitiveRequestHeaders<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveRequestHeaders {
            inner,
            headers: self.headers.clone(),
        }
    }
}

/// Mark request headers as [sensitive].
///
/// See the [module docs](crate::sensitive_headers) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveRequestHeaders<S> {
    inner: S,
    headers: Arc<[HeaderName]>,
}

impl<S> SetSensitiveRequestHeaders<S> {
    /// Create a new [`SetSensitiveRequestHeaders`].
    pub fn new<I>(inner: S, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        let headers = headers.into_iter().collect::<Vec<_>>();
        Self::from_shared(inner, headers.into())
    }

    /// Create a new [`SetSensitiveRequestHeaders`] from a shared slice of headers.
    pub fn from_shared(inner: S, headers: Arc<[HeaderName]>) -> Self {
        Self { inner, headers }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `SetSensitiveRequestHeaders` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer<I>(headers: I) -> SetSensitiveRequestHeadersLayer
    where
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveRequestHeadersLayer::new(headers)
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for SetSensitiveRequestHeaders<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let headers = req.headers_mut();
        for header in &*self.headers {
            if let http::header::Entry::Occupied(mut entry) = headers.entry(header) {
                for value in entry.iter_mut() {
                    value.set_sensitive(true);
                }
            }
        }

        self.inner.call(req)
    }
}

/// Mark response headers as [sensitive].
///
/// Produces [`SetSensitiveResponseHeaders`] services.
///
/// See the [module docs](crate::sensitive_headers) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveResponseHeadersLayer {
    headers: Arc<[HeaderName]>,
}

impl SetSensitiveResponseHeadersLayer {
    /// Create a new [`SetSensitiveResponseHeadersLayer`].
    pub fn new<I>(headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        let headers = headers.into_iter().collect::<Vec<_>>();
        Self::from_shared(headers.into())
    }

    /// Create a new [`SetSensitiveResponseHeadersLayer`] from a shared slice of headers.
    pub fn from_shared(headers: Arc<[HeaderName]>) -> Self {
        Self { headers }
    }
}

impl<S> Layer<S> for SetSensitiveResponseHeadersLayer {
    type Service = SetSensitiveResponseHeaders<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SetSensitiveResponseHeaders {
            inner,
            headers: self.headers.clone(),
        }
    }
}

/// Mark response headers as [sensitive].
///
/// See the [module docs](crate::sensitive_headers) for more details.
///
/// [sensitive]: https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.set_sensitive
#[derive(Clone, Debug)]
pub struct SetSensitiveResponseHeaders<S> {
    inner: S,
    headers: Arc<[HeaderName]>,
}

impl<S> SetSensitiveResponseHeaders<S> {
    /// Create a new [`SetSensitiveResponseHeaders`].
    pub fn new<I>(inner: S, headers: I) -> Self
    where
        I: IntoIterator<Item = HeaderName>,
    {
        let headers = headers.into_iter().collect::<Vec<_>>();
        Self::from_shared(inner, headers.into())
    }

    /// Create a new [`SetSensitiveResponseHeaders`] from a shared slice of headers.
    pub fn from_shared(inner: S, headers: Arc<[HeaderName]>) -> Self {
        Self { inner, headers }
    }

    define_inner_service_accessors!();

    /// Returns a new [`Layer`] that wraps services with a `SetSensitiveResponseHeaders` middleware.
    ///
    /// [`Layer`]: tower_layer::Layer
    pub fn layer<I>(headers: I) -> SetSensitiveResponseHeadersLayer
    where
        I: IntoIterator<Item = HeaderName>,
    {
        SetSensitiveResponseHeadersLayer::new(headers)
    }
}

impl<ReqBody, ResBody, S> Service<Request<ReqBody>> for SetSensitiveResponseHeaders<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = SetSensitiveResponseHeadersResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        SetSensitiveResponseHeadersResponseFuture {
            future: self.inner.call(req),
            headers: self.headers.clone(),
        }
    }
}

pin_project! {
    /// Response future for [`SetSensitiveResponseHeaders`].
    #[derive(Debug)]
    pub struct SetSensitiveResponseHeadersResponseFuture<F> {
        #[pin]
        future: F,
        headers: Arc<[HeaderName]>,
    }
}

impl<F, ResBody, E> Future for SetSensitiveResponseHeadersResponseFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = ready!(this.future.poll(cx)?);

        let headers = res.headers_mut();
        for header in &**this.headers {
            if let http::header::Entry::Occupied(mut entry) = headers.entry(header) {
                for value in entry.iter_mut() {
                    value.set_sensitive(true);
                }
            }
        }

        Poll::Ready(Ok(res))
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use http::header;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn multiple_value_header() {
        async fn response_set_cookie(req: http::Request<()>) -> Result<http::Response<()>, ()> {
            let mut iter = req.headers().get_all(header::COOKIE).iter().peekable();

            assert!(iter.peek().is_some());

            for value in iter {
                assert!(value.is_sensitive())
            }

            let mut resp = http::Response::new(());
            resp.headers_mut().append(
                header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/html"),
            );
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-1"),
            );
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-2"),
            );
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-3"),
            );
            Ok(resp)
        }

        let mut service = ServiceBuilder::new()
            .layer(SetSensitiveRequestHeadersLayer::new(vec![header::COOKIE]))
            .layer(SetSensitiveResponseHeadersLayer::new(vec![
                header::SET_COOKIE,
            ]))
            .service_fn(response_set_cookie);

        let mut req = http::Request::new(());
        req.headers_mut()
            .append(header::COOKIE, http::HeaderValue::from_static("cookie+1"));
        req.headers_mut()
            .append(header::COOKIE, http::HeaderValue::from_static("cookie+2"));

        let resp = service.ready().await.unwrap().call(req).await.unwrap();

        assert!(!resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .is_sensitive());

        let mut iter = resp.headers().get_all(header::SET_COOKIE).iter().peekable();

        assert!(iter.peek().is_some());

        for value in iter {
            assert!(value.is_sensitive())
        }
    }

    #[tokio::test]
    async fn response_layer_marks_repeated_values_but_keeps_bytes_and_order() {
        async fn response_two_set_cookies(
            req: http::Request<()>,
        ) -> Result<http::Response<()>, ()> {
            // The response layer must not touch the request, even for header
            // names it was configured with.
            let cookies: Vec<_> = req.headers().get_all(header::COOKIE).iter().collect();
            assert_eq!(cookies.len(), 2);
            assert!(cookies.iter().all(|value| !value.is_sensitive()));
            assert_eq!(cookies[0].as_bytes(), b"cookie+1");
            assert_eq!(cookies[1].as_bytes(), b"cookie+2");

            let mut resp = http::Response::new(());
            resp.headers_mut().append(
                header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/html"),
            );
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-1"),
            );
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-2"),
            );
            Ok(resp)
        }

        let mut service = ServiceBuilder::new()
            .layer(SetSensitiveResponseHeadersLayer::new(vec![
                header::SET_COOKIE,
                header::COOKIE,
            ]))
            .service_fn(response_two_set_cookies);

        let mut req = http::Request::new(());
        req.headers_mut()
            .append(header::COOKIE, http::HeaderValue::from_static("cookie+1"));
        req.headers_mut()
            .append(header::COOKIE, http::HeaderValue::from_static("cookie+2"));

        let resp = service.ready().await.unwrap().call(req).await.unwrap();

        // A normal header that wasn't listed is left alone.
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap();
        assert!(!content_type.is_sensitive());
        assert_eq!(content_type.as_bytes(), b"text/html");

        // Every repeated value of a listed header is marked, while the raw
        // bytes and their order are preserved.
        let set_cookies: Vec<_> = resp.headers().get_all(header::SET_COOKIE).iter().collect();
        assert_eq!(set_cookies.len(), 2);
        assert_eq!(set_cookies[0].as_bytes(), b"cookie-1");
        assert_eq!(set_cookies[1].as_bytes(), b"cookie-2");
        assert!(set_cookies.iter().all(|value| value.is_sensitive()));
    }

    #[tokio::test]
    async fn response_layer_preserves_inner_service_error() {
        async fn failing_service(_req: http::Request<()>) -> Result<http::Response<()>, String> {
            Err("something went wrong".to_owned())
        }

        let mut service = ServiceBuilder::new()
            .layer(SetSensitiveResponseHeadersLayer::new(vec![
                header::SET_COOKIE,
            ]))
            .service_fn(failing_service);

        let err = service
            .ready()
            .await
            .unwrap()
            .call(http::Request::new(()))
            .await
            .unwrap_err();

        assert_eq!(err, "something went wrong");
    }

    #[tokio::test]
    async fn without_the_layer_response_headers_are_unchanged() {
        async fn response_two_set_cookies(
            _req: http::Request<()>,
        ) -> Result<http::Response<()>, ()> {
            let mut resp = http::Response::new(());
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-1"),
            );
            resp.headers_mut().append(
                header::SET_COOKIE,
                http::HeaderValue::from_static("cookie-2"),
            );
            Ok(resp)
        }

        // No `SetSensitiveResponseHeadersLayer` is applied.
        let mut service = ServiceBuilder::new().service_fn(response_two_set_cookies);

        let resp = service
            .ready()
            .await
            .unwrap()
            .call(http::Request::new(()))
            .await
            .unwrap();

        let set_cookies: Vec<_> = resp.headers().get_all(header::SET_COOKIE).iter().collect();
        assert_eq!(set_cookies.len(), 2);
        assert_eq!(set_cookies[0].as_bytes(), b"cookie-1");
        assert_eq!(set_cookies[1].as_bytes(), b"cookie-2");
        assert!(set_cookies.iter().all(|value| !value.is_sensitive()));
    }
}
