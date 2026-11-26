use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// A canned `internal-api`. Runs as a real axum server on an ephemeral port so
/// `ApiClient` exercises its true HTTP and deserialization path - no trait
/// indirection introduced into production code just for tests.
pub struct StubApi {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    unmatched: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct StubState {
    routes: Arc<HashMap<String, Value>>,
    requests: Arc<Mutex<Vec<String>>>,
    unmatched: Arc<Mutex<Vec<String>>>,
}

impl StubApi {
    /// `routes` maps an exact path, including any query string, to its JSON body.
    /// There is no path-only fallback: a request carrying a query string needs a
    /// route registered with that same query string.
    pub async fn start(routes: Vec<(&str, Value)>) -> Self {
        let table: HashMap<String, Value> = routes
            .into_iter()
            .map(|(path, body)| (path.to_string(), body))
            .collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let unmatched = Arc::new(Mutex::new(Vec::new()));

        let state = StubState {
            routes: Arc::new(table),
            requests: Arc::clone(&requests),
            unmatched: Arc::clone(&unmatched),
        };

        let app = Router::new().fallback(serve).with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub api should bind an ephemeral port");
        let addr = listener.local_addr().expect("stub api should have an addr");

        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self {
            endpoint: format!("http://{addr}"),
            requests,
            unmatched,
        }
    }

    pub fn endpoint(&self) -> String {
        self.endpoint.clone()
    }

    /// Paths requested, in order. Assert on this to catch a tool fetching more
    /// (or less) than it should.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("requests lock poisoned")
            .clone()
    }

    /// Paths requested that had no fixture, in order. Tools that read through
    /// `get_optional` or swallow errors turn the 404 into an ordinary result, so
    /// this is the only reliable signal that a fixture is missing.
    pub fn unmatched(&self) -> Vec<String> {
        self.unmatched
            .lock()
            .expect("unmatched lock poisoned")
            .clone()
    }
}

async fn serve(State(state): State<StubState>, req: Request) -> Response {
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    state
        .requests
        .lock()
        .expect("requests lock poisoned")
        .push(path.clone());

    match state.routes.get(&path) {
        Some(body) => axum::Json(body.clone()).into_response(),
        // A missing fixture must look like a missing fixture, not like empty data.
        None => {
            state
                .unmatched
                .lock()
                .expect("unmatched lock poisoned")
                .push(path.clone());
            (
                StatusCode::NOT_FOUND,
                format!("stub api has no fixture for `{path}`"),
            )
                .into_response()
        }
    }
}
