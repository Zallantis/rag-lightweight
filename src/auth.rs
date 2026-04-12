use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct AuthState {
    expected_token: Option<String>,
}

impl AuthState {
    pub fn new(token: Option<String>) -> Self {
        Self {
            expected_token: token,
        }
    }
}

pub async fn require_bearer_token(
    State(state): State<AuthState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let Some(expected) = &state.expected_token else {
        return next.run(request).await;
    };

    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let Some(token) = token else {
        return unauthorized();
    };

    let a = expected.as_bytes();
    let b = token.as_bytes();
    if a.len() != b.len() || a.ct_eq(b).unwrap_u8() != 1 {
        return unauthorized();
    }

    next.run(request).await
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use tower::ServiceExt;

    fn app(token: Option<&str>) -> Router {
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .route_layer(middleware::from_fn_with_state(
                AuthState::new(token.map(String::from)),
                require_bearer_token,
            ))
    }

    #[tokio::test]
    async fn auth_disabled_passes_through() {
        let resp = app(None)
            .oneshot(Request::get("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_header_returns_401() {
        let resp = app(Some("secret"))
            .oneshot(Request::get("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_format_returns_401() {
        let resp = app(Some("secret"))
            .oneshot(
                Request::get("/test")
                    .header("authorization", "Basic abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_token_returns_401() {
        let resp = app(Some("secret"))
            .oneshot(
                Request::get("/test")
                    .header("authorization", "Bearer wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn correct_token_passes_through() {
        let resp = app(Some("secret"))
            .oneshot(
                Request::get("/test")
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
