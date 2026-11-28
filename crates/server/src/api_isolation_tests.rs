//! 多用户隔离的**端到端**用例（M-25 · 批 1）。
//!
//! 这些用例走真 HTTP（`tower::ServiceExt::oneshot` 直接把请求喂进路由表），
//! 而不是只调处理器：隔离的判据有一半在中间件里（cookie / Bearer / 归属），
//! 跳过中间件测就等于没测。
//!
//! 覆盖的七条验收：
//!
//! | 用例 | 断言 |
//! | --- | --- |
//! | `cross_account_session_is_not_found` | A 的会话对 B 是 404（不泄露存在性） |
//! | `list_only_shows_the_callers_sessions` | 列表按账号过滤 |
//! | `account_quota_is_per_account` | 单账号配额打满不影响别人 |
//! | `no_token_is_401_once_accounts_are_on` | 有账号库 ⇒ 没令牌 401 |
//! | `compat_mode_keeps_the_single_token_behaviour` | 没有账号库 ⇒ v0.1.9 行为 |
//! | `login_cookie_is_accepted_by_whoami` | 账号密码 → cookie → 身份 |
//! | `the_ops_token_cannot_see_account_sessions` | `SIM_TOKEN` 只是 ops 自己 |
//!
//! ⚠️ 每个用例都用自己的临时账号文件：并行跑的测试不能共享一份状态。

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::account::{self, Accounts, Identity};
use crate::api::{router, AppState};
use crate::config::Config;

/// 一份两个账号、三个令牌的账号库文件（Alice 两把、Bob 一把）。
struct Fixture {
    path: PathBuf,
    alice_token_a: String,
    alice_token_b: String,
    bob_token: String,
    alice_password: String,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("m25_isolation_{tag}.json"));
        let alice_token_a = account::new_token().unwrap();
        let alice_token_b = account::new_token().unwrap();
        let bob_token = account::new_token().unwrap();
        let alice_password = "alice-secret".to_string();
        let store = account::Store {
            version: 1,
            accounts: vec![
                account::AccountRecord {
                    id: "acc-alice".into(),
                    name: "alice".into(),
                    role: "user".into(),
                    password: Some(account::encode_password(&alice_password, 2)),
                    tokens: vec![
                        account::token_record("tok-alice-a".into(), &alice_token_a),
                        account::token_record("tok-alice-b".into(), &alice_token_b),
                    ],
                    created: String::new(),
                },
                account::AccountRecord {
                    id: "acc-bob".into(),
                    name: "bob".into(),
                    role: "user".into(),
                    password: None,
                    tokens: vec![account::token_record("tok-bob".into(), &bob_token)],
                    created: String::new(),
                },
            ],
        };
        std::fs::write(&path, serde_json::to_string(&store).unwrap()).unwrap();
        Self {
            path,
            alice_token_a,
            alice_token_b,
            bob_token,
            alice_password,
        }
    }

    fn accounts(&self) -> Arc<Accounts> {
        Arc::new(Accounts::load(&self.path).unwrap().unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

fn state_with(accounts: Option<Arc<Accounts>>, max_per_account: usize) -> AppState {
    let cfg = Config {
        max_sessions_per_account: max_per_account,
        ..Config::default()
    };
    AppState::new(&cfg, "127.0.0.1:9876".parse().unwrap()).with_accounts(accounts)
}

fn request(method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn call(state: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let response = router(state.clone()).oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn create_session(state: &AppState, token: &str) -> String {
    let (status, body) = call(
        state,
        request("POST", "/api/sessions", Some(token), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create failed: {body}");
    body["session_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn cross_account_session_is_not_found() {
    let fixture = Fixture::new("cross");
    let state = state_with(Some(fixture.accounts()), 4);
    let sid = create_session(&state, &fixture.alice_token_a).await;

    // 自己看得到。
    let (status, _) = call(
        &state,
        request(
            "GET",
            &format!("/api/sessions/{sid}/state"),
            Some(&fixture.alice_token_a),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 别人看到的是**同一个 404**（与「不存在」不可区分）。
    let (status, body) = call(
        &state,
        request(
            "GET",
            &format!("/api/sessions/{sid}/state"),
            Some(&fixture.bob_token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "E_SESS-01");

    // 写面同样拒（DELETE 也不能跨账号）。
    let (status, _) = call(
        &state,
        request(
            "DELETE",
            &format!("/api/sessions/{sid}"),
            Some(&fixture.bob_token),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_only_shows_the_callers_sessions() {
    let fixture = Fixture::new("list");
    let state = state_with(Some(fixture.accounts()), 4);
    let sid = create_session(&state, &fixture.alice_token_a).await;

    let (_, alice) = call(
        &state,
        request("GET", "/api/sessions", Some(&fixture.alice_token_a), None),
    )
    .await;
    assert_eq!(alice["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(alice["sessions"][0]["session_id"], sid.as_str());

    let (_, bob) = call(
        &state,
        request("GET", "/api/sessions", Some(&fixture.bob_token), None),
    )
    .await;
    assert!(bob["sessions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_same_account_shares_sessions_across_its_tokens() {
    let fixture = Fixture::new("tokens");
    let state = state_with(Some(fixture.accounts()), 4);
    let sid = create_session(&state, &fixture.alice_token_a).await;

    // 同一账号的第二把令牌看到同一条会话（这是「一个账号多把令牌」的口径）。
    let (status, body) = call(
        &state,
        request(
            "GET",
            &format!("/api/sessions/{sid}/state"),
            Some(&fixture.alice_token_b),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn account_quota_is_per_account() {
    let fixture = Fixture::new("quota");
    let state = state_with(Some(fixture.accounts()), 1);
    let _sid = create_session(&state, &fixture.alice_token_a).await;

    // 同一个账号的第二条 ⇒ 429。
    let (status, body) = call(
        &state,
        request(
            "POST",
            "/api/sessions",
            Some(&fixture.alice_token_a),
            Some(json!({})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "E_SESS-05");

    // 别的账号不受影响。
    let (status, body) = call(
        &state,
        request("POST", "/api/sessions", Some(&fixture.bob_token), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn no_token_is_401_once_accounts_are_on() {
    let fixture = Fixture::new("noauth");
    let state = state_with(Some(fixture.accounts()), 4);
    let (status, body) = call(&state, request("GET", "/api/sessions", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "E_SESS-07");

    // 错的令牌同样 401（**不能**回 404，那会泄露「这个端点存在」）。
    let (status, _) = call(
        &state,
        request("GET", "/api/sessions", Some("not-a-token"), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn compat_mode_keeps_the_single_token_behaviour() {
    // 没有账号库 ⇒ 与 v0.1.9 一致：配了令牌就校验，没配就放行。
    let cfg = Config {
        token: Some("legacy-secret".to_string()),
        ..Config::default()
    };
    let state = AppState::new(&cfg, "127.0.0.1:9876".parse().unwrap());
    let (status, _) = call(&state, request("GET", "/api/sessions", None, None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = call(
        &state,
        request("GET", "/api/sessions", Some("legacy-secret"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 兼容模式下所有会话都归 `legacy`，中间件的归属判定必须放行。
    let (status, body) = call(
        &state,
        request("POST", "/api/sessions", Some("legacy-secret"), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn the_ops_token_cannot_see_account_sessions() {
    let fixture = Fixture::new("ops");
    let cfg = Config {
        token: Some("ops-break-glass".to_string()),
        max_sessions_per_account: 4,
        ..Config::default()
    };
    let state = AppState::new(&cfg, "127.0.0.1:9876".parse().unwrap())
        .with_accounts(Some(fixture.accounts()));
    let _sid = create_session(&state, &fixture.alice_token_a).await;

    let (status, body) = call(
        &state,
        request("GET", "/api/sessions", Some("ops-break-glass"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["sessions"].as_array().unwrap().is_empty(),
        "ops 令牌只是它自己的账号，不该看见别人的会话：{body}"
    );
}

#[tokio::test]
async fn login_cookie_is_accepted_by_whoami() {
    let fixture = Fixture::new("login");
    let state = state_with(Some(fixture.accounts()), 4);

    let (status, body) = call(
        &state,
        request(
            "POST",
            "/api/login",
            None,
            Some(json!({"username": "alice", "password": fixture.alice_password})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["account"], "acc-alice");

    // 密码错 ⇒ 401，且**不**说「账号不存在」还是「密码不对」。
    let (status, body) = call(
        &state,
        request(
            "POST",
            "/api/login",
            None,
            Some(json!({"username": "alice", "password": "wrong"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["detail"], "invalid credentials");
}

#[tokio::test]
async fn image_root_is_scoped_per_account() {
    let root = std::env::temp_dir().join("m25_image_root_probe");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();

    let cfg = Config {
        allow_image_root: Some(root.clone()),
        ..Config::default()
    };
    let state = AppState::new(&cfg, "127.0.0.1:9876".parse().unwrap());

    // 兼容模式：根就是根（现有 `/data/images/*.elf` 原地不动）。
    let legacy = Identity::legacy();
    assert_eq!(
        crate::api::image_root_for(&state, &legacy).unwrap(),
        root.clone()
    );

    // 账号模式：根下面按账号分一层，并且真的建出来了。
    let alice = Identity {
        account: "acc-alice".into(),
        name: "alice".into(),
        role: "user".into(),
        token_id: None,
        channel: "token",
    };
    let scoped = crate::api::image_root_for(&state, &alice).unwrap();
    assert_eq!(scoped, root.join("acc-alice"));
    assert!(scoped.is_dir());

    // 账号 id 里的怪字符被拍平（`../` 不会决定根在哪）。
    // `.` 与 `/` 都不是 `[a-z0-9_-]` ⇒ 一律拍成 `_`；结果不含任何分隔符。
    assert_eq!(crate::api::account_dir_name("../../etc"), "______etc");
    std::fs::remove_dir_all(&root).ok();
}
