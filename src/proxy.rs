use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{
    body::Incoming,
    header::HOST,
    http::{HeaderValue, StatusCode},
    server::conn::http1,
    service::service_fn,
    Request, Response, Uri,
};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
};
use tokio::{net::TcpListener, sync::RwLock};
use tracing::{error, info};

use crate::{
    errors::{DevhostError, Result},
    router::RouteTable,
};

pub type SharedRoutes = Arc<RwLock<RouteTable>>;
type ProxyBody = BoxBody<Bytes, hyper::Error>;
type ProxyResponse = Response<ProxyBody>;

pub async fn serve(addr: SocketAddr, routes: SharedRoutes) -> Result<()> {
    let listener = TcpListener::bind(addr).await.map_err(|err| {
        if addr.port() < 1024 && err.kind() == std::io::ErrorKind::PermissionDenied {
            DevhostError::InvalidConfig(format!(
                "listening on {addr} requires root privileges; run `cargo build` and then `sudo target/debug/devhost serve --config devhost.toml`"
            ))
        } else {
            err.into()
        }
    })?;
    info!("devhost listening on http://{}", listener.local_addr()?);
    serve_listener(listener, routes).await
}

pub async fn serve_listener(listener: TcpListener, routes: SharedRoutes) -> Result<()> {
    let client: Client<HttpConnector, Incoming> =
        Client::builder(TokioExecutor::new()).build_http();

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let client = client.clone();
        let routes = routes.clone();

        tokio::spawn(async move {
            let service =
                service_fn(move |request| handle_request(request, routes.clone(), client.clone()));

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                error!(%peer_addr, error = %err, "connection failed");
            }
        });
    }
}

async fn handle_request(
    request: Request<Incoming>,
    routes: SharedRoutes,
    client: Client<HttpConnector, Incoming>,
) -> std::result::Result<ProxyResponse, Infallible> {
    match forward_request(request, routes, client).await {
        Ok(response) => Ok(response),
        Err(err) => {
            error!(error = %err, "upstream request failed");
            Ok(text_response(
                StatusCode::BAD_GATEWAY,
                "bad gateway: upstream request failed\n",
            ))
        }
    }
}

async fn forward_request(
    mut request: Request<Incoming>,
    routes: SharedRoutes,
    client: Client<HttpConnector, Incoming>,
) -> Result<ProxyResponse> {
    let Some(host) = request.headers().get(HOST).and_then(header_to_str) else {
        return Ok(text_response(StatusCode::NOT_FOUND, "not found\n"));
    };

    let Some(target) = routes.read().await.resolve(host) else {
        return Ok(text_response(StatusCode::NOT_FOUND, "not found\n"));
    };

    let upstream_uri = upstream_uri(&target, request.uri())?;
    *request.uri_mut() = upstream_uri;

    let response = client.request(request).await?;
    Ok(response.map(|body| body.boxed()))
}

fn header_to_str(value: &HeaderValue) -> Option<&str> {
    value.to_str().ok()
}

fn upstream_uri(target: &Uri, original: &Uri) -> Result<Uri> {
    let scheme = target.scheme_str().unwrap_or("http");
    let authority = target
        .authority()
        .ok_or_else(|| DevhostError::InvalidConfig("route target is missing authority".into()))?;
    let path_and_query = original
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    Ok(format!("{scheme}://{authority}{path_and_query}").parse::<Uri>()?)
}

fn text_response(status: StatusCode, body: &'static str) -> ProxyResponse {
    Response::builder()
        .status(status)
        .body(boxed_full(body))
        .expect("status and static body should build")
}

fn boxed_full(body: impl Into<Bytes>) -> ProxyBody {
    Full::new(body.into())
        .map_err(|never| match never {})
        .boxed()
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http_body_util::{BodyExt, Full};
    use hyper::{Method, Request, Response, StatusCode};

    use crate::config::RouteConfig;

    use super::*;

    async fn start_upstream() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = TokioIo::new(stream);
                let service = service_fn(|request: Request<Incoming>| async move {
                    let method = request.method().clone();
                    let path = request
                        .uri()
                        .path_and_query()
                        .map(|pq| pq.as_str().to_string())
                        .unwrap_or_else(|| "/".to_string());
                    let body = request.into_body().collect().await.unwrap().to_bytes();
                    let response = format!("{method} {path} {}", String::from_utf8_lossy(&body));

                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(response))))
                });

                tokio::spawn(async move {
                    let _ = http1::Builder::new().serve_connection(io, service).await;
                });
            }
        });

        addr
    }

    async fn start_proxy(routes: RouteTable) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let routes = Arc::new(RwLock::new(routes));

        tokio::spawn(async move {
            let _ = serve_listener(listener, routes).await;
        });

        addr
    }

    fn route(host: &str, target: String) -> RouteConfig {
        RouteConfig {
            host: host.to_string(),
            target: target.parse::<Uri>().unwrap(),
        }
    }

    #[tokio::test]
    async fn forwards_method_path_query_and_body() {
        let upstream = start_upstream().await;
        let proxy = start_proxy(RouteTable::new(&[route(
            "app.test",
            format!("http://{upstream}"),
        )]))
        .await;

        let client: Client<HttpConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build_http();
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("http://{proxy}/hello?name=devhost"))
            .header(HOST, "app.test")
            .body(Full::new(Bytes::from("ping")))
            .unwrap();

        let response = client.request(request).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(
            String::from_utf8_lossy(&body),
            "POST /hello?name=devhost ping"
        );
    }

    #[tokio::test]
    async fn returns_404_for_unknown_host() {
        let proxy = start_proxy(RouteTable::new(&[])).await;
        let client: Client<HttpConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build_http();
        let request = Request::builder()
            .uri(format!("http://{proxy}/"))
            .header(HOST, "missing.test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = client.request(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn returns_502_for_unavailable_upstream() {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let unused = reserved.local_addr().unwrap();
        drop(reserved);

        let proxy = start_proxy(RouteTable::new(&[route(
            "app.test",
            format!("http://{unused}"),
        )]))
        .await;
        let client: Client<HttpConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build_http();
        let request = Request::builder()
            .uri(format!("http://{proxy}/"))
            .header(HOST, "app.test")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let response = client.request(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
