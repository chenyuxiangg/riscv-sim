//! 账号、令牌与浏览器会话（M-25 · 多用户隔离）。
//!
//! ## 它解决的问题
//!
//! v0.1.9 之前只有一个 `SIM_TOKEN`：任何人拿到它就是这个进程的**全部权限**
//! —— 能看所有人的会话、装任何人的镜像。多用户上线后需要：
//!
//! - **一个账号**（人）**多个令牌**（客户端）⇒ 令牌被吊销时只废掉那一把；
//! - 浏览器走 `POST /api/login` 换一个 `HttpOnly` cookie，令牌**不进 URL**；
//! - MCP 客户端带 `Authorization: Bearer <token>`，服务端把它解析成账号。
//!
//! ## 三条硬边界
//!
//! 1. **令牌只存哈希**：`accounts.json` 里的 `hash` 是 `sha256(token)` 的十六
//!    进制串；文件被读走也换不回令牌。`hint` 只留前 4 个字符，给运维认人用。
//! 2. **密码只存派生值**：`pbkdf2_sha256$<iter>$<salt>$<hash>`，校验时重算。
//!    实现只有一份（[`pbkdf2_sha256`]），测试用 RFC 向量钉住。
//! 3. **兼容模式**：`accounts.json` 不存在 ⇒ [`Accounts::load`] 回 `None`，
//!    服务端退回 v0.1.9 的单 `SIM_TOKEN` 行为（逐字节不变）。这条保证上线
//!    当天不会把现有链路弄死。
//!
//! ⚠️ **cookie 会话只在内存里**：进程重启后浏览器要重新登录。这是刻意的
//! —— 落盘的会话表要处理失效、轮换与并发写，收益不抵风险。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 兼容模式的账号名。`accounts.json` 缺席时所有请求都归到它名下 ⇒ 与
/// v0.1.9 的单租户行为一致（没有第二套授权，也没有隔离）。
pub const LEGACY_ACCOUNT: &str = "legacy";

/// 浏览器会话 cookie 的名字。
pub const COOKIE_NAME: &str = "sim_auth";

/// 浏览器会话的有效期（12 小时）。
pub const COOKIE_TTL: Duration = Duration::from_secs(12 * 3600);

/// `SIM_TOKEN` 这把应急令牌映射到的账号名（运维直连用）。
pub const OPS_ACCOUNT: &str = "ops";

/// 一次身份解析的结果。中间件把它塞进请求扩展，处理器只读它。
#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    /// 账号 id（`accounts.json` 里的稳定标识）。
    pub account: String,
    /// 账号名（人读的）。
    pub name: String,
    /// 角色：`admin` / `user`。**只作审计与展示**，判定一律按账号。
    pub role: String,
    /// 用哪把令牌进来的（cookie 会话与兼容模式为 `None`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    /// 从哪条通道进来：`cookie` / `token` / `legacy`。
    pub channel: &'static str,
}

impl Identity {
    /// 兼容模式的身份（`accounts.json` 缺席）。
    pub fn legacy() -> Self {
        Self {
            account: LEGACY_ACCOUNT.to_string(),
            name: LEGACY_ACCOUNT.to_string(),
            role: "admin".to_string(),
            token_id: None,
            channel: "legacy",
        }
    }

    /// `SIM_TOKEN` 的应急身份：能登录、能建会话，但**看不见任何账号的会话**
    /// （它自己就是 `ops` 这个账号）。
    pub fn ops() -> Self {
        Self {
            account: OPS_ACCOUNT.to_string(),
            name: OPS_ACCOUNT.to_string(),
            role: "admin".to_string(),
            token_id: None,
            channel: "token",
        }
    }
}

// --------------------------------------------------------------------------- 磁盘形状

/// `accounts.json` 的顶层形状。字段全部带默认值 ⇒ ⇒ 手写一份最小文件也能读。
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Store {
    /// 文件版本。留着给以后加字段用；当前只认 1。
    #[serde(default = "one")]
    pub version: u32,
    /// 全部账号。
    #[serde(default)]
    pub accounts: Vec<AccountRecord>,
}

fn one() -> u32 {
    1
}

/// 一个账号：密码（浏览器） + 一堆令牌（MCP）。
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct AccountRecord {
    /// 稳定 id（十六进制 8 位）。**不要**用账号名当 id：改名会连带丢会话。
    pub id: String,
    /// 账号名（登录用）。
    pub name: String,
    /// `admin` / `user`。
    #[serde(default = "user_role")]
    pub role: String,
    /// `pbkdf2_sha256$<iter>$<salt_b64url>$<hash_b64url>`；`None` ⇒ 这个账号
    /// 不能用密码登录（只发过令牌）。
    #[serde(default)]
    pub password: Option<String>,
    /// 令牌表。
    #[serde(default)]
    pub tokens: Vec<TokenRecord>,
    /// 创建时刻（ISO8601，仅展示）。
    #[serde(default)]
    pub created: String,
}

fn user_role() -> String {
    "user".to_string()
}

/// 一把令牌。**`hash` 之外的一切都是给人看的**。
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct TokenRecord {
    /// 令牌 id（`tok-xxxx`），吊销与审计按它定位。
    pub id: String,
    /// 明文前 4 位，给运维认人（**不足以还原令牌**）。
    #[serde(default)]
    pub hint: String,
    /// `sha256(token)` 的十六进制串（小写）。
    pub hash: String,
    /// 创建时刻（ISO8601）。
    #[serde(default)]
    pub created: String,
    /// 是否已吊销。吊销后立即 401，**不删记录**（审计要能看到曾经有过）。
    #[serde(default)]
    pub revoked: bool,
}

// --------------------------------------------------------------------------- 运行时

struct CookieEntry {
    account: String,
    name: String,
    role: String,
    expires: Instant,
}

/// 账号库 + 浏览器会话表。
pub struct Accounts {
    path: PathBuf,
    store: Store,
    cookies: Mutex<HashMap<String, CookieEntry>>,
}

impl std::fmt::Debug for Accounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accounts")
            .field("path", &self.path)
            .field("accounts", &self.store.accounts.len())
            .finish()
    }
}

impl Accounts {
    /// 从磁盘读。**文件缺席 ⇒ `Ok(None)`**（兼容模式）；存在但读不动 ⇒ `Err`。
    ///
    /// ⚠️ 缺席与畸形**必须分开**：前者是「还没启用账号」，后者是「你写错了
    /// 一个字节」，混在一起会让一个错字静默地退回单租户。
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(format!("cannot read accounts file {}: {err}", path.display()))
            }
        };
        let store: Store = serde_json::from_str(&raw)
            .map_err(|err| format!("accounts file {} is not valid JSON: {err}", path.display()))?;
        if store.version != 1 {
            return Err(format!(
                "accounts file {} has unknown version {} (this build understands 1)",
                path.display(),
                store.version
            ));
        }
        for account in &store.accounts {
            if account.id.trim().is_empty() || account.name.trim().is_empty() {
                return Err(format!(
                    "accounts file {} has an account without id/name",
                    path.display()
                ));
            }
        }
        Ok(Some(Self {
            path: path.to_path_buf(),
            store,
            cookies: Mutex::new(HashMap::new()),
        }))
    }

    /// 账号数（`/healthz` 与启动日志用，**不列名字**）。
    pub fn account_count(&self) -> usize {
        self.store.accounts.len()
    }

    /// 令牌总数（含已吊销）。
    pub fn token_count(&self) -> usize {
        self.store.accounts.iter().map(|a| a.tokens.len()).sum()
    }

    /// 令牌 → 身份。**已吊销的令牌一律不认**。
    pub fn authenticate_token(&self, presented: &str) -> Option<Identity> {
        if presented.trim().is_empty() {
            return None;
        }
        let digest = hex(&sha256(presented.as_bytes()));
        for account in &self.store.accounts {
            for token in &account.tokens {
                if token.revoked {
                    continue;
                }
                if constant_time_eq(token.hash.as_bytes(), digest.as_bytes()) {
                    return Some(Identity {
                        account: account.id.clone(),
                        name: account.name.clone(),
                        role: account.role.clone(),
                        token_id: Some(token.id.clone()),
                        channel: "token",
                    });
                }
            }
        }
        None
    }

    /// 账号名 + 密码 → 身份。**用户名错与密码错回同一个 `None`**（调用方只
    /// 能回一句 401，不能泄露「这个账号存在」）。
    pub fn login(&self, name: &str, password: &str) -> Option<Identity> {
        let account = self
            .store
            .accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name.trim()))?;
        let encoded = account.password.as_deref()?;
        if !verify_password(encoded, password) {
            return None;
        }
        Some(Identity {
            account: account.id.clone(),
            name: account.name.clone(),
            role: account.role.clone(),
            token_id: None,
            channel: "cookie",
        })
    }

    /// 发一个浏览器会话，回 cookie 值（32 字节随机，base64url）。
    ///
    /// ⚠️ 顺手清掉已过期的条目：这张表只装浏览器会话，不扫也长不大，但
    /// 「永不清理」是个会被人复制的坏例子。
    pub fn issue_cookie(&self, identity: &Identity) -> String {
        let raw = random_bytes(32);
        let value = b64url(&raw);
        let mut cookies = match self.cookies.lock() {
            Ok(cookies) => cookies,
            Err(poisoned) => poisoned.into_inner(),
        };
        let now = Instant::now();
        cookies.retain(|_, entry| entry.expires > now);
        cookies.insert(
            value.clone(),
            CookieEntry {
                account: identity.account.clone(),
                name: identity.name.clone(),
                role: identity.role.clone(),
                expires: now + COOKIE_TTL,
            },
        );
        value
    }

    /// cookie 值 → 身份。
    pub fn resolve_cookie(&self, value: &str) -> Option<Identity> {
        let mut cookies = match self.cookies.lock() {
            Ok(cookies) => cookies,
            Err(poisoned) => poisoned.into_inner(),
        };
        let now = Instant::now();
        let entry = cookies.get(value)?;
        if entry.expires <= now {
            cookies.remove(value);
            return None;
        }
        let identity = Identity {
            account: entry.account.clone(),
            name: entry.name.clone(),
            role: entry.role.clone(),
            token_id: None,
            channel: "cookie",
        };
        // 滑动续期：还在用的人不该被一次 12 小时掐断。
        if let Some(entry) = cookies.get_mut(value) {
            entry.expires = now + COOKIE_TTL;
        }
        Some(identity)
    }

    /// 注销一个浏览器会话。**幂等**：不存在的值当作已经注销。
    pub fn logout(&self, value: &str) {
        let mut cookies = match self.cookies.lock() {
            Ok(cookies) => cookies,
            Err(poisoned) => poisoned.into_inner(),
        };
        cookies.remove(value);
    }
}

// --------------------------------------------------------------------------- 密码

/// 生成一份可落盘的密码派生串（`tools/accounts.py` 用的是**同一套**参数）。
///
/// ⚠️ 运行时**没有任何路径**用它（建账号的是 `tools/accounts.py`）⇒ 只在
/// 测试里编进来，免得「两套密码生成」漂成两个真相。
#[cfg(test)]
pub fn encode_password(password: &str, iterations: u32) -> String {
    let salt = random_bytes(16);
    let hash = pbkdf2_sha256(password.as_bytes(), &salt, iterations, 32);
    format!(
        "pbkdf2_sha256${iterations}${}${}",
        b64url(&salt),
        b64url(&hash)
    )
}

/// 校验派生串。任何解析失败都回 `false`（**不 panic**：这是网络输入侧）。
pub fn verify_password(encoded: &str, password: &str) -> bool {
    let mut parts = encoded.split('$');
    if parts.next() != Some("pbkdf2_sha256") {
        return false;
    }
    let iterations: u32 = match parts.next().and_then(|raw| raw.parse().ok()) {
        Some(value) => value,
        None => return false,
    };
    let salt = match parts.next().and_then(b64url_decode) {
        Some(value) => value,
        None => return false,
    };
    let expected = match parts.next().and_then(b64url_decode) {
        Some(value) => value,
        None => return false,
    };
    if iterations == 0 || expected.is_empty() {
        return false;
    }
    let actual = pbkdf2_sha256(password.as_bytes(), &salt, iterations, expected.len());
    constant_time_eq(&actual, &expected)
}

/// PBKDF2-HMAC-SHA256（RFC 8018）。**唯一**的实现点。
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len + 32);
    let mut block: u32 = 1;
    while out.len() < out_len {
        let mut message = salt.to_vec();
        message.extend_from_slice(&block.to_be_bytes());
        let mut u = hmac_sha256(password, &message);
        let mut acc = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for (a, b) in acc.iter_mut().zip(u.iter()) {
                *a ^= *b;
            }
        }
        out.extend_from_slice(&acc);
        block += 1;
    }
    out.truncate(out_len);
    out
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= block[i];
        opad[i] ^= block[i];
    }
    let mut inner = Vec::with_capacity(64 + message.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(message);
    let inner_hash = sha256(&inner);
    let mut outer = Vec::with_capacity(96);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 常数时间比较：长度不同直接 `false`（长度不是秘密），逐字节异或累加。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// --------------------------------------------------------------------------- 编码与随机

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url（无填充）。
pub fn b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(triple >> 18) as usize & 0x3f] as char);
        out.push(B64[(triple >> 12) as usize & 0x3f] as char);
        if chunk.len() > 1 {
            out.push(B64[(triple >> 6) as usize & 0x3f] as char);
        }
        if chunk.len() > 2 {
            out.push(B64[triple as usize & 0x3f] as char);
        }
    }
    out
}

/// base64url 解码。**非法输入回 `None`**，不 panic。
pub fn b64url_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 + 2);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for byte in bytes {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// 从 `/dev/urandom` 取随机字节。**读不到就回空** —— 调用方据此拒绝服务
/// （比拿时间戳冒充随机安全得多）。
pub fn random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut out = vec![0u8; n];
    let mut file = match std::fs::File::open("/dev/urandom") {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    match file.read_exact(&mut out) {
        Ok(()) => out,
        Err(_) => Vec::new(),
    }
}

/// 生成一把令牌明文（32 字节随机 ⇒ 43 字符 base64url）。
#[cfg(test)]
pub fn new_token() -> Option<String> {
    let bytes = random_bytes(32);
    if bytes.len() != 32 {
        return None;
    }
    Some(b64url(&bytes))
}

/// 令牌的落盘形态：`sha256` 十六进制 + 前 4 位提示。
#[cfg(test)]
pub fn token_record(id: String, token: &str) -> TokenRecord {
    TokenRecord {
        id,
        hint: token.chars().take(4).collect(),
        hash: hex(&sha256(token.as_bytes())),
        created: String::new(),
        revoked: false,
    }
}

/// 从 `Cookie` 头里取 `sim_auth` 的值。
pub fn cookie_from_header(header: Option<&str>) -> Option<String> {
    let header = header?;
    for pair in header.split(';') {
        let mut it = pair.splitn(2, '=');
        let name = it.next().unwrap_or("").trim();
        let value = it.next().unwrap_or("").trim();
        if name == COOKIE_NAME && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// `Set-Cookie` 的取值。`secure` 只在真正 HTTPS 的请求上给 —— 本机 `http`
/// 调试时带上 `Secure` 会让浏览器**直接不存**，人看到的是「登录成功了但
/// 每次都说未登录」。
pub fn set_cookie(value: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        COOKIE_TTL.as_secs()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// 清 cookie 的取值（登出）。
pub fn clear_cookie(secure: bool) -> String {
    let mut cookie = format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir();
        dir.join(name)
    }

    #[test]
    fn pbkdf2_matches_the_rfc_vectors() {
        // RFC 7914 附录里的 PBKDF2-HMAC-SHA256 向量（c=1 / c=2）。
        assert_eq!(
            hex(&pbkdf2_sha256(b"password", b"salt", 1, 32)),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        assert_eq!(
            hex(&pbkdf2_sha256(b"password", b"salt", 2, 32)),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn base64url_round_trips() {
        for len in [1usize, 2, 3, 4, 5, 31, 32] {
            let raw = random_bytes(len);
            if raw.len() != len {
                continue;
            }
            let text = b64url(&raw);
            assert_eq!(b64url_decode(&text).unwrap(), raw);
        }
        assert!(b64url_decode("!!").is_none());
    }

    #[test]
    fn password_round_trip_and_tampering() {
        let encoded = encode_password("correct horse", 2);
        assert!(verify_password(&encoded, "correct horse"));
        assert!(!verify_password(&encoded, "correct horsE"));
        assert!(!verify_password("not-a-phc-string", "correct horse"));
        assert!(!verify_password("pbkdf2_sha256$0$$", "x"));
    }

    #[test]
    fn a_token_resolves_to_its_account_and_revocation_wins() {
        let path = temp_path("m25_token_probe.json");
        let token = new_token().unwrap();
        let store = Store {
            version: 1,
            accounts: vec![AccountRecord {
                id: "a-01".into(),
                name: "alice".into(),
                role: "user".into(),
                password: None,
                tokens: vec![token_record("tok-1".into(), &token)],
                created: String::new(),
            }],
        };
        std::fs::write(&path, serde_json::to_string(&store).unwrap()).unwrap();
        let accounts = Accounts::load(&path).unwrap().unwrap();
        let identity = accounts.authenticate_token(&token).unwrap();
        assert_eq!(identity.account, "a-01");
        assert_eq!(identity.token_id.as_deref(), Some("tok-1"));
        assert!(accounts.authenticate_token("garbage").is_none());

        // 吊销后立刻不认（记录还在，审计能看见）。
        let mut revoked = store;
        revoked.accounts[0].tokens[0].revoked = true;
        std::fs::write(&path, serde_json::to_string(&revoked).unwrap()).unwrap();
        let accounts = Accounts::load(&path).unwrap().unwrap();
        assert!(accounts.authenticate_token(&token).is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_is_compat_mode_and_a_broken_file_is_an_error() {
        let path = temp_path("m25_absent_accounts.json");
        std::fs::remove_file(&path).ok();
        assert!(Accounts::load(&path).unwrap().is_none());

        std::fs::write(&path, "{ not json").unwrap();
        assert!(Accounts::load(&path).is_err());
        std::fs::write(&path, r#"{"version":9,"accounts":[]}"#).unwrap();
        assert!(Accounts::load(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_cookie_session_expires_and_logs_out() {
        let path = temp_path("m25_cookie_probe.json");
        std::fs::write(&path, r#"{"version":1,"accounts":[]}"#).unwrap();
        let accounts = Accounts::load(&path).unwrap().unwrap();
        let identity = Identity {
            account: "a-01".into(),
            name: "alice".into(),
            role: "user".into(),
            token_id: None,
            channel: "cookie",
        };
        let cookie = accounts.issue_cookie(&identity);
        assert_eq!(accounts.resolve_cookie(&cookie).unwrap().account, "a-01");
        accounts.logout(&cookie);
        assert!(accounts.resolve_cookie(&cookie).is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn cookies_are_parsed_from_a_dirty_header() {
        assert_eq!(
            cookie_from_header(Some("a=1; sim_auth=tok-abc; b=2")).as_deref(),
            Some("tok-abc")
        );
        assert!(cookie_from_header(Some("a=1")).is_none());
        assert!(cookie_from_header(None).is_none());
    }

    #[test]
    fn login_needs_both_the_name_and_the_password() {
        let path = temp_path("m25_login_probe.json");
        let password = encode_password("s3cret", 2);
        let store = Store {
            version: 1,
            accounts: vec![AccountRecord {
                id: "a-01".into(),
                name: "alice".into(),
                role: "admin".into(),
                password: Some(password),
                tokens: Vec::new(),
                created: String::new(),
            }],
        };
        std::fs::write(&path, serde_json::to_string(&store).unwrap()).unwrap();
        let accounts = Accounts::load(&path).unwrap().unwrap();
        assert!(accounts.login("alice", "wrong").is_none());
        assert!(accounts.login("bob", "s3cret").is_none());
        assert_eq!(accounts.login("ALICE", "s3cret").unwrap().account, "a-01");
        std::fs::remove_file(&path).ok();
    }
}
