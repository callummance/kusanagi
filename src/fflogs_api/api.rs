use hyper::body::Buf;
use hyper::{Body, Client, Method, Request};
use hyper_tls::HttpsConnector;

use http::uri::{Authority, Builder, PathAndQuery, Scheme, Uri};

use serde::Serialize;

use log::{info, warn};

pub const FFLOGS_SCHEME: &'static str = "https";
pub const FFLOGS_AUTHORITY: &'static str = "www.fflogs.com:443";

pub struct FFLogsApiClient {
    hyper_client: Client<HttpsConnector<hyper::client::connect::HttpConnector>, Body>,
    api_key: String,
}

impl FFLogsApiClient {
    pub async fn run_request(&self, uri: Uri) -> Result<String, ApiError> {
        let target_uri = uri.to_string();
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::empty())
            .map_err(|err| ApiError::RequestConstructionError(err))?;
        let res = self
            .hyper_client
            .request(request)
            .await
            .map_err(|err| ApiError::RequestError(err))?;
        if !res.status().is_success() {
            let status = res.status().as_u16();
            warn!(
                "Request to api endpoint found at {:?} return non-success response {}",
                target_uri, status
            );
            let body = hyper::body::aggregate(res)
                .await
                .map_err(|err| ApiError::ApiReturnedError((status, err.to_string())))?
                .to_bytes();
            let body_str = String::from_utf8_lossy(&body);
            return Err(ApiError::ApiReturnedError((status, body_str.to_string())));
        }
        let body = hyper::body::aggregate(res)
            .await
            .map_err(|err| ApiError::RequestError(err))?
            .to_bytes();
        let body_string = String::from_utf8_lossy(&body);
        info!("Successfully requested data from endpoint {}", target_uri);
        return Ok(body_string.to_string());
    }

    pub fn api_key(&self) -> &str {
        return &self.api_key;
    }
}

/// Creates a new FFLogsApiClient struct with the given API key and
/// an HTTPS client
pub fn new_fflogs_api_client(api_key: &str) -> FFLogsApiClient {
    let https = HttpsConnector::new();
    let hyper_client = Client::builder().build::<_, Body>(https);

    let new_client = FFLogsApiClient {
        hyper_client: hyper_client,
        api_key: api_key.to_owned(),
    };
    info!(
        "Created new FFLogs API Client with API key {}",
        trunc_api_key(api_key)
    );
    return new_client;
}

fn trunc_api_key(full_key: &str) -> String {
    let mut truncated: String = full_key.chars().take(4).collect();
    truncated.push_str("...");
    return truncated;
}

/// Type representing error which can occur whilst running or building an FFLogs API request
#[derive(Debug)]
pub enum ApiError {
    QueryStringGenerationError(String),
    UrlFragmentGenerationError(http::uri::InvalidUri),
    UrlGenerationError(http::Error),
    RequestConstructionError(http::Error),
    RequestError(hyper::Error),
    ApiReturnedError((u16, String)),
    ResponseFormatError(serde_json::Error),
    NotImplementedError,
}

/// Given a path and a struct implementing ToQueryString returns a string
/// conaining the path and query which can then be used to create a uri.
pub fn to_path_and_query<T>(path: &str, query: T) -> Result<String, ApiError>
where
    T: Serialize,
{
    let query_string = serde_urlencoded::to_string(query);
    return query_string
        .map(|qstr| format!("{}?{}", path, qstr))
        .map_err(|err| ApiError::QueryStringGenerationError(err.to_string()));
}

/// Given a scheme, base url, resource path and struct implementing ToQueryString,
/// attempts to create a Uri obeject.
pub fn to_uri<T>(scheme: &str, authority: &str, path: &str, query: T) -> Result<Uri, ApiError>
where
    T: Serialize,
{
    let sch: Scheme = scheme
        .parse()
        .map_err(|err| ApiError::UrlFragmentGenerationError(err))?;
    let auth: Authority = authority
        .parse()
        .map_err(|err| ApiError::UrlFragmentGenerationError(err))?;
    let pandq: PathAndQuery = to_path_and_query(path, query)?
        .parse()
        .map_err(|err| ApiError::UrlFragmentGenerationError(err))?;

    let uri = Builder::new()
        .scheme(sch)
        .authority(auth)
        .path_and_query(pandq)
        .build()
        .map_err(|err| ApiError::UrlGenerationError(err));
    return uri;
}

/// Generates a Uri representing a given request to the FFLogs API
pub fn fflogs_request<T>(path: &str, query: T) -> Result<Uri, ApiError>
where
    T: Serialize,
{
    to_uri(FFLOGS_SCHEME, FFLOGS_AUTHORITY, path, query)
}
