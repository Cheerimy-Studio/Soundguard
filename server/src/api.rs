use std::time::Duration as StdDuration;

use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, FromRequestParts, Path, Query, State,
    },
    http::{header, request::Parts, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post, put},
    Json, Router,
};
use base64::Engine as _;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    auth::{
        hash_password, hash_token, issue_user_token, new_device_id, new_device_token, new_user_id,
        secure_eq, validate_password, verify_password, verify_token, CurrentUser,
    },
    error::AppError,
    models::{
        round1, Alert, AlertKind, CallRecord, ChangePasswordRequest, ControlRequest,
        CreateCallRequest, CreateDeviceRequest, CreateDeviceResponse, CreateUserRequest, Device,
        DeviceControlResponse, DeviceListResponse, DeviceSettingsUpdate, DeviceTokenResponse,
        DeviceView, HeartbeatRequest, HeartbeatResponse, LiveRequest, LoginRequest, LoginResponse,
        NotificationUpdate, OverviewResponse, Reading, ReportResult, Settings, SettingsResponse,
        Stats, StopIntentRequest, StopIntentResponse, TelemetryRequest, TelemetryResponse,
        UpdateDeviceRequest, UpdateUserRequest, User, UserView, VerifyPinRequest,
        VerifyPinResponse,
    },
    notify::{alert_attachment, email_shell, html_escape, initial_email_status, spawn_email},
    reports::{downsample, readings_csv, send_daily_report},
    state::AppState,
    store::safe_filename,
};

pub struct Admin {
    pub user: CurrentUser,
}

impl Admin {
    pub fn is_super(&self) -> bool {
        self.user.is_super()
    }

    pub fn require_super(&self) -> Result<(), AppError> {
        if self.is_super() {
            Ok(())
        } else {
            Err(AppError::forbidden("只有超级管理员可以执行此操作"))
        }
    }

    pub fn can_access_device(&self, device_id: &str) -> bool {
        self.user.can_access_device(device_id)
    }

    pub fn ensure_device_access(&self, device_id: &str) -> Result<(), AppError> {
        if self.can_access_device(device_id) {
            Ok(())
        } else {
            Err(AppError::forbidden("无权访问该设备"))
        }
    }
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value: &HeaderValue| value.to_str().ok())
            .unwrap_or_default();
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .unwrap_or_default()
            .trim();
        let claims = verify_token(&state.config, token).ok_or_else(AppError::unauthorized)?;
        let user = state
            .store
            .user(&claims.sub)
            .await
            .ok_or_else(AppError::unauthorized)?;
        if user.disabled || user.token_version != claims.ver || user.role.as_str() != claims.role {
            return Err(AppError::unauthorized());
        }
        Ok(Admin {
            user: CurrentUser::from_user(&user),
        })
    }
}

struct DeviceAuth;
impl FromRequestParts<AppState> for DeviceAuth {
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get("x-device-token")
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty() && v.len() <= 512)
            .ok_or_else(AppError::unauthorized)?;
        let hash = hash_token(token);
        if !state.store.authenticates_token(&hash).await {
            return Err(AppError::unauthorized());
        }
        Ok(Self)
    }
}

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route(
            "/auth/login",
            post(login).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/auth/me", get(me))
        .route("/auth/password", post(change_password))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", put(update_user).delete(delete_user))
        .route("/notifications", put(update_notifications))
        .route("/devices/{id}/settings", put(update_device_settings))
        .route("/devices", get(list_devices).post(create_device))
        .route("/devices/{id}", put(update_device).delete(delete_device))
        .route("/devices/{id}/token", post(rotate_device_token))
        .route("/devices/{id}/status", get(device_status))
        .route("/overview", get(overview))
        .route("/readings", get(list_readings))
        .route("/readings/export", get(export_readings))
        .route("/alerts", get(list_alerts))
        .route("/alerts/{id}/ack", post(ack_alert))
        .route("/alerts/{id}/evidence", get(get_evidence))
        .route("/settings", get(get_settings).put(update_settings))
        .route("/device/control", post(control_device))
        .route("/reports/send", post(send_report))
        .route("/test/alert", post(test_alert))
        .route(
            "/calls",
            get(list_calls)
                .post(create_call)
                .layer(DefaultBodyLimit::max(17 * 1024 * 1024)),
        )
        .route("/calls/{id}/audio", get(get_call_audio))
        .route("/ws", get(admin_ws))
        .route("/device/heartbeat", post(device_heartbeat))
        .route(
            "/device/telemetry",
            post(device_telemetry).layer(DefaultBodyLimit::max(9 * 1024 * 1024)),
        )
        .route("/device/live", post(device_live))
        .route("/device/stop-intent", post(device_stop_intent))
        .route("/device/local-stop", post(device_local_stop))
        .route("/device/verify-pin", post(device_verify_pin))
        .route("/device/calls", get(device_calls))
        .route("/device/calls/{id}/audio", get(device_call_audio))
        .route("/device/ws", get(device_ws))
        .fallback(api_not_found)
        .layer(axum::middleware::from_fn(api_response_headers))
        .with_state(state.clone());

    let index = state.config.web_index();
    let static_files =
        ServeDir::new(&state.config.web_dist).not_found_service(ServeFile::new(index));

    let cors = if state.config.cors_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(HeaderValue::from_static(
                "https://appassets.androidplatform.net",
            ))
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<HeaderValue> = state
            .config
            .cors_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    Router::new()
        .nest("/api", api)
        .fallback_service(static_files)
        .layer(CompressionLayer::new())
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

async fn api_response_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn api_not_found() -> AppError {
    AppError::not_found("接口不存在")
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "name": "SoundGuard",
        "version": env!("CARGO_PKG_VERSION"),
        "time": Utc::now(),
        "smtpConfigured": state.mailer.configured(),
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    if request.username.len() > 128 || request.password.len() > 512 {
        return Err(AppError::bad_request("用户名或密码过长"));
    }
    let username = request.username.trim();
    let user = state.store.user_by_username(username).await;
    // Unrecognized names share one bucket, so invented names cannot fill the
    // lockout table and deny service to real accounts.
    let account_key = user
        .as_ref()
        .map(|u| u.id.clone())
        .unwrap_or_else(|| "unknown".into());
    let password_permit = state
        .password_jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::new(StatusCode::TOO_MANY_REQUESTS, "登录繁忙，请稍后再试"))?;
    {
        let mut guard = state.login_guard.lock().unwrap_or_else(|p| p.into_inner());
        let now = std::time::Instant::now();
        guard.accounts.retain(|_, entry| {
            entry
                .touched
                .is_some_and(|t| now.duration_since(t).as_secs() < 600)
        });
        if guard.accounts.len() >= 4096 && !guard.accounts.contains_key(&account_key) {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "登录繁忙，请稍后再试",
            ));
        }
        let entry = guard.accounts.entry(account_key.clone()).or_default();
        if entry.locked_until.is_some_and(|until| until > now) {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "此账号登录错误过多，请 5 分钟后再试",
            ));
        }
        if entry.locked_until.take().is_some() {
            entry.failures = 0;
        }
        entry.touched = Some(now);
    }

    let hash = user
        .as_ref()
        .filter(|u| !u.disabled)
        .map(|u| u.password_hash.clone())
        .unwrap_or_else(|| state.dummy_password_hash.as_ref().clone());
    let password = request.password.clone();
    let valid_hash = tokio::task::spawn_blocking(move || {
        let _permit = password_permit;
        verify_password(&hash, &password)
    })
    .await
    .map_err(|_| AppError::internal("密码验证失败"))?;
    let valid = valid_hash && user.as_ref().is_some_and(|u| !u.disabled);
    if !valid {
        let locked = {
            let mut guard = state
                .login_guard
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let entry = guard.accounts.entry(account_key.clone()).or_default();
            entry.failures = entry.failures.saturating_add(1);
            entry.touched = Some(std::time::Instant::now());
            if entry.failures >= 5 {
                entry.failures = 0;
                entry.locked_until = Some(std::time::Instant::now() + StdDuration::from_secs(300));
                true
            } else {
                false
            }
        };
        tokio::time::sleep(StdDuration::from_millis(350)).await;
        warn!("Rejected login attempt for username={username}");
        if locked {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "此账号登录错误过多，请 5 分钟后再试",
            ));
        }
        return Err(AppError::unauthorized());
    }
    {
        let mut guard = state
            .login_guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.accounts.remove(&account_key);
    }
    let mut user = user.expect("validated user");
    let now = Utc::now();
    state
        .store
        .mutate(|snapshot| {
            if let Some(target) = snapshot.users.iter_mut().find(|item| item.id == user.id) {
                if target.disabled || target.token_version != user.token_version {
                    return false;
                }
                target.last_login_at = Some(now);
                user = target.clone();
                return true;
            }
            false
        })
        .await?
        .then_some(())
        .ok_or_else(AppError::unauthorized)?;
    // The Android app keeps its session locally and needs a longer-lived token
    // so it can stay signed in across restarts.
    let ttl_hours = if request.remember {
        24 * 30
    } else {
        state.config.jwt_ttl_hours
    };
    let (token, expires_at) = issue_user_token(&state.config, &user, ttl_hours)?;
    info!(
        "User session created: {} ({})",
        user.username,
        user.role.as_str()
    );
    Ok(Json(LoginResponse {
        token,
        expires_at,
        user: user.view(),
    }))
}

async fn me(admin: Admin, State(state): State<AppState>) -> Result<Json<UserView>, AppError> {
    let user = state
        .store
        .user(&admin.user.id)
        .await
        .ok_or_else(AppError::unauthorized)?;
    Ok(Json(user.view()))
}

async fn password_hash_job(state: &AppState, password: &str) -> Result<String, AppError> {
    let permit = state
        .password_jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::new(StatusCode::TOO_MANY_REQUESTS, "密码处理繁忙，请稍后重试"))?;
    let password = password.to_string();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password(&password)
    })
    .await
    .map_err(|_| AppError::internal("密码处理失败"))?
    .map_err(AppError::from)
}

async fn password_verify_job(
    state: &AppState,
    hash: String,
    password: String,
) -> Result<bool, AppError> {
    if password.len() > 512 {
        return Ok(false);
    }
    let permit = state
        .password_jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::new(StatusCode::TOO_MANY_REQUESTS, "密码处理繁忙，请稍后重试"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&hash, &password)
    })
    .await
    .map_err(|_| AppError::internal("密码处理失败"))
}

async fn change_password(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<Value>, AppError> {
    validate_password(&request.new_password).map_err(AppError::bad_request)?;
    let user = state
        .store
        .user(&admin.user.id)
        .await
        .ok_or_else(AppError::unauthorized)?;
    if !password_verify_job(&state, user.password_hash.clone(), request.current_password).await? {
        return Err(AppError::bad_request("当前密码不正确"));
    }
    let hash = password_hash_job(&state, &request.new_password).await?;
    state
        .store
        .mutate(|snapshot| {
            if let Some(target) = snapshot.users.iter_mut().find(|item| item.id == user.id) {
                target.password_hash = hash;
                target.token_version = target.token_version.saturating_add(1);
            }
        })
        .await?;
    Ok(Json(json!({ "ok": true, "reauthenticate": true })))
}

fn validate_email(raw: &str) -> Result<String, AppError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(AppError::bad_request("请填写接收邮箱"));
    }
    let recipients = crate::config::parse_recipients(value);
    if recipients.len() != 1 || recipients[0] != value {
        return Err(AppError::bad_request("接收邮箱格式不正确"));
    }
    let Some((local, domain)) = value.split_once('@') else {
        return Err(AppError::bad_request("接收邮箱格式不正确"));
    };
    if value.parse::<lettre::Address>().is_err()
        || local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return Err(AppError::bad_request("接收邮箱格式不正确"));
    }
    Ok(recipients[0].clone())
}

fn validate_username(username: &str) -> Result<String, AppError> {
    let value = username.trim();
    let length = value.chars().count();
    if !(3..=32).contains(&length) {
        return Err(AppError::bad_request("用户名长度需为 3–32 个字符"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(AppError::bad_request(
            "用户名只能包含字母、数字、下划线、短横线或点",
        ));
    }
    Ok(value.to_string())
}

async fn list_users(
    admin: Admin,
    State(state): State<AppState>,
) -> Result<Json<Vec<UserView>>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    Ok(Json(snapshot.users.iter().map(User::view).collect()))
}

async fn create_user(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<Json<UserView>, AppError> {
    admin.require_super()?;
    let username = validate_username(&request.username)?;
    let email = validate_email(&request.email)?;
    validate_password(&request.password).map_err(AppError::bad_request)?;
    let password_hash = password_hash_job(&state, &request.password).await?;
    let device_ids = if request.role.is_super() {
        Vec::new()
    } else {
        let snapshot = state.store.snapshot().await;
        let mut ids: Vec<String> = request
            .device_ids
            .into_iter()
            .filter(|id| snapshot.devices.iter().any(|device| &device.id == id))
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };
    let user = state
        .store
        .mutate(|snapshot| {
            if snapshot
                .users
                .iter()
                .any(|user| user.username.eq_ignore_ascii_case(&username))
            {
                return Err(AppError::new(StatusCode::CONFLICT, "用户名已存在"));
            }
            let user = User {
                id: new_user_id(),
                username,
                email,
                password_hash,
                role: request.role,
                device_ids,
                disabled: false,
                token_version: 1,
                created_at: Utc::now(),
                last_login_at: None,
            };
            snapshot.users.push(user.clone());
            Ok(user)
        })
        .await??;
    Ok(Json(user.view()))
}

async fn update_user(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateUserRequest>,
) -> Result<Json<UserView>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    let target = snapshot
        .users
        .iter()
        .find(|user| user.id == id)
        .cloned()
        .ok_or_else(|| AppError::not_found("用户不存在"))?;
    let next_role = request.role.clone().unwrap_or(target.role.clone());
    let next_disabled = request.disabled.unwrap_or(target.disabled);
    let role_or_status_changed = next_role != target.role || next_disabled != target.disabled;
    let active_supers = snapshot
        .users
        .iter()
        .filter(|user| user.role.is_super() && !user.disabled)
        .count();
    if target.role.is_super()
        && !target.disabled
        && active_supers <= 1
        && (!next_role.is_super() || next_disabled)
    {
        return Err(AppError::bad_request("必须保留至少一个可用的超级管理员"));
    }
    if target.id == admin.user.id && next_disabled {
        return Err(AppError::bad_request("不能停用当前登录账号"));
    }
    let username = match request.username {
        Some(value) => Some(validate_username(&value)?),
        None => None,
    };
    let new_hash = match request.password {
        Some(value) => {
            validate_password(&value).map_err(AppError::bad_request)?;
            Some(password_hash_job(&state, &value).await?)
        }
        None => None,
    };
    let email = match request.email {
        Some(value) => Some(validate_email(&value)?),
        None => None,
    };
    let next_email = email.clone().unwrap_or_else(|| target.email.clone());
    if next_email.trim().is_empty() {
        return Err(AppError::bad_request("请填写接收邮箱"));
    }
    let device_ids = if next_role.is_super() {
        Vec::new()
    } else if let Some(ids) = request.device_ids {
        let mut ids: Vec<String> = ids
            .into_iter()
            .filter(|id| snapshot.devices.iter().any(|device| &device.id == id))
            .collect();
        ids.sort();
        ids.dedup();
        ids
    } else {
        target.device_ids.clone()
    };
    let updated = state
        .store
        .mutate(|snapshot| {
            if let Some(name) = &username {
                if snapshot
                    .users
                    .iter()
                    .any(|user| user.id != id && user.username.eq_ignore_ascii_case(name))
                {
                    return Err(AppError::new(StatusCode::CONFLICT, "用户名已存在"));
                }
            }
            let active_supers = snapshot
                .users
                .iter()
                .filter(|u| u.role.is_super() && !u.disabled)
                .count();
            if snapshot
                .users
                .iter()
                .any(|u| u.id == id && u.role.is_super() && !u.disabled)
                && active_supers <= 1
                && (!next_role.is_super() || next_disabled)
            {
                return Err(AppError::bad_request("必须保留至少一个可用的超级管理员"));
            }
            let user = snapshot
                .users
                .iter_mut()
                .find(|user| user.id == id)
                .ok_or_else(|| AppError::not_found("用户不存在"))?;
            if let Some(name) = username {
                user.username = name;
            }
            if let Some(email) = email {
                user.email = email;
            }
            let assignments_changed = user.device_ids != device_ids;
            user.role = next_role;
            user.device_ids = device_ids;
            user.disabled = next_disabled;
            if let Some(hash) = new_hash {
                user.password_hash = hash;
                user.token_version = user.token_version.saturating_add(1);
            } else if role_or_status_changed || assignments_changed {
                user.token_version = user.token_version.saturating_add(1);
            }
            Ok(user.view())
        })
        .await??;
    Ok(Json(updated))
}

async fn delete_user(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    admin.require_super()?;
    if id == admin.user.id {
        return Err(AppError::bad_request("不能删除当前登录账号"));
    }
    state
        .store
        .mutate(|snapshot| {
            let target = snapshot
                .users
                .iter()
                .find(|user| user.id == id)
                .cloned()
                .ok_or_else(|| AppError::not_found("用户不存在"))?;
            let active_supers = snapshot
                .users
                .iter()
                .filter(|user| user.role.is_super() && !user.disabled)
                .count();
            if target.role.is_super() && !target.disabled && active_supers <= 1 {
                return Err(AppError::bad_request("必须保留至少一个可用的超级管理员"));
            }
            snapshot.users.retain(|user| user.id != id);
            Ok(())
        })
        .await??;
    Ok(Json(json!({ "ok": true })))
}

async fn update_notifications(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<NotificationUpdate>,
) -> Result<Json<SettingsResponse>, AppError> {
    admin.require_super()?;
    let mut settings = state.store.settings().await;
    settings.notification_enabled = request.enabled;
    settings.notification_title = request.title;
    settings.notification_content = request.content;
    settings
        .validate_and_normalize()
        .map_err(AppError::bad_request)?;
    let saved = state
        .store
        .mutate(|snapshot| {
            snapshot.settings = settings.clone();
            snapshot.settings.clone()
        })
        .await?;
    state
        .hub
        .broadcast_event("settings", serde_json::to_value(&saved).unwrap_or_default());
    Ok(Json(SettingsResponse {
        settings: saved,
        smtp_configured: state.mailer.configured(),
        ice_servers: state.config.ice_servers.clone(),
    }))
}

fn device_views(
    state: &crate::models::PersistedState,
    now: DateTime<Utc>,
    user: &CurrentUser,
) -> Vec<DeviceView> {
    state
        .devices
        .iter()
        .filter(|device| user.can_access_device(&device.id))
        .map(|device| {
            let offline =
                crate::models::effective_settings(&state.settings, device).offline_timeout_secs;
            device.view(now, offline)
        })
        .collect()
}

fn resolve_optional_device(
    admin: &Admin,
    snapshot: &crate::models::PersistedState,
    requested: Option<&str>,
) -> Result<Option<String>, AppError> {
    if admin.is_super() {
        return Ok(requested.map(ToString::to_string));
    }
    if let Some(id) = requested {
        admin.ensure_device_access(id)?;
        return Ok(Some(id.to_string()));
    }
    let accessible: Vec<&Device> = snapshot
        .devices
        .iter()
        .filter(|device| admin.can_access_device(&device.id))
        .collect();
    match accessible.as_slice() {
        [device] => Ok(Some(device.id.clone())),
        [] => Err(AppError::forbidden("尚未分配可管理的设备")),
        _ => Err(AppError::bad_request("请选择要查看的设备")),
    }
}

fn find_device<'a>(
    state: &'a crate::models::PersistedState,
    device_id: &str,
) -> Option<&'a Device> {
    state.devices.iter().find(|device| device.id == device_id)
}

async fn authenticate_device(
    state: &AppState,
    headers: &HeaderMap,
    device_id: &str,
) -> Result<Device, AppError> {
    let token = headers
        .get("x-device-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if token.is_empty() {
        return Err(AppError::unauthorized());
    }
    let device = state
        .store
        .device(device_id)
        .await
        .ok_or_else(AppError::unauthorized)?;
    if device.token_hash.is_empty() || !secure_eq(&device.token_hash, &hash_token(token)) {
        return Err(AppError::unauthorized());
    }
    Ok(device)
}

async fn authenticate_device_query(
    state: &AppState,
    token: &str,
    device_id: &str,
) -> Result<Device, AppError> {
    if token.is_empty() {
        return Err(AppError::unauthorized());
    }
    let device = state
        .store
        .device(device_id)
        .await
        .ok_or_else(AppError::unauthorized)?;
    if device.token_hash.is_empty() || !secure_eq(&device.token_hash, &hash_token(token)) {
        return Err(AppError::unauthorized());
    }
    Ok(device)
}

async fn list_devices(
    admin: Admin,
    State(state): State<AppState>,
) -> Result<Json<DeviceListResponse>, AppError> {
    let snapshot = state.store.snapshot().await;
    Ok(Json(DeviceListResponse {
        devices: device_views(&snapshot, Utc::now(), &admin.user),
    }))
}

fn validate_device_name(
    devices: &[Device],
    name: &str,
    except_id: Option<&str>,
) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::bad_request("设备名称需为 1–60 个字符"));
    }
    if devices.iter().any(|device| {
        Some(device.id.as_str()) != except_id && device.name.eq_ignore_ascii_case(name)
    }) {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "设备名称已存在，请使用唯一名称",
        ));
    }
    Ok(name.to_string())
}

async fn create_device(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<CreateDeviceRequest>,
) -> Result<Json<CreateDeviceResponse>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    let name = validate_device_name(&snapshot.devices, &request.name, None)?;
    let device_id = match request.device_id {
        Some(raw) if !raw.trim().is_empty() => {
            let id = raw.trim().to_string();
            if id.len() < 3
                || id.len() > 64
                || !id
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            {
                return Err(AppError::bad_request(
                    "设备编号需为 3–64 位字母、数字、- 或 _",
                ));
            }
            if snapshot.devices.iter().any(|device| device.id == id) {
                return Err(AppError::new(StatusCode::CONFLICT, "设备编号已存在"));
            }
            id
        }
        _ => new_device_id(),
    };
    let token = new_device_token();
    let token_hash = hash_token(&token);
    let created_at = Utc::now();
    let device = state
        .store
        .mutate(|state| {
            let name = validate_device_name(&state.devices, &name, None)?;
            if state.devices.iter().any(|d| d.id == device_id) {
                return Err(AppError::new(StatusCode::CONFLICT, "设备编号已存在"));
            }
            let device = Device::new(device_id, name, token_hash);
            state.devices.push(device.clone());
            if !admin.is_super() {
                if let Some(user) = state.users.iter_mut().find(|user| user.id == admin.user.id) {
                    user.device_ids.push(device.id.clone());
                    user.device_ids.sort();
                    user.device_ids.dedup();
                }
            }
            Ok(device)
        })
        .await??;
    let device_settings = crate::models::effective_settings(&snapshot.settings, &device);
    let response = CreateDeviceResponse {
        device: device.view(created_at, device_settings.offline_timeout_secs),
        token,
    };
    let latest = state.store.snapshot().await;
    state.hub.broadcast_event(
        "devices",
        serde_json::to_value(device_views(
            &latest,
            Utc::now(),
            &CurrentUser {
                id: String::new(),
                role: crate::models::UserRole::SuperAdmin,
                device_ids: vec![],
            },
        ))
        .unwrap_or_default(),
    );
    Ok(Json(response))
}

async fn update_device(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateDeviceRequest>,
) -> Result<Json<DeviceView>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    if find_device(&snapshot, &id).is_none() {
        return Err(AppError::not_found("设备不存在"));
    }
    let name = validate_device_name(&snapshot.devices, &request.name, Some(&id))?;
    let now = Utc::now();
    let updated = state
        .store
        .mutate(|state| {
            let name = validate_device_name(&state.devices, &name, Some(&id))?;
            let target = state.devices.iter_mut().find(|device| device.id == id);
            Ok::<_, AppError>(target.map(|device| {
                device.name = name;
                let offline =
                    crate::models::effective_settings(&state.settings, device).offline_timeout_secs;
                device.view(now, offline)
            }))
        })
        .await??
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    state
        .hub
        .broadcast_event("device", serde_json::to_value(&updated).unwrap_or_default());
    Ok(Json(updated))
}

async fn update_device_settings(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<DeviceSettingsUpdate>,
) -> Result<Json<DeviceView>, AppError> {
    // Device policy (threshold / cooldown / offline / evidence) may be tuned by
    // the device admin assigned to this node; global defaults stay super-only.
    admin.ensure_device_access(&id)?;
    let mut settings = request.into_settings();
    settings
        .validate_and_normalize()
        .map_err(AppError::bad_request)?;
    let now = Utc::now();
    let updated = state
        .store
        .mutate(|snapshot| {
            let target = snapshot.devices.iter_mut().find(|device| device.id == id)?;
            target.settings = settings;
            let offline =
                crate::models::effective_settings(&snapshot.settings, target).offline_timeout_secs;
            Some(target.view(now, offline))
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    state
        .hub
        .broadcast_event("device", serde_json::to_value(&updated).unwrap_or_default());
    Ok(Json(updated))
}

async fn delete_device(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    admin.require_super()?;
    let removed = state
        .store
        .mutate(|state| {
            if state.devices.len() <= 1 {
                return false;
            }
            let before = state.devices.len();
            state.devices.retain(|device| device.id != id);
            for user in &mut state.users {
                user.device_ids.retain(|device_id| device_id != &id);
            }
            before != state.devices.len()
        })
        .await?;
    if !removed {
        return Err(AppError::bad_request("至少保留一个设备，或设备不存在"));
    }
    let latest = state.store.snapshot().await;
    state.hub.broadcast_event(
        "devices",
        serde_json::to_value(device_views(
            &latest,
            Utc::now(),
            &CurrentUser {
                id: String::new(),
                role: crate::models::UserRole::SuperAdmin,
                device_ids: vec![],
            },
        ))
        .unwrap_or_default(),
    );
    Ok(Json(json!({ "ok": true })))
}

async fn rotate_device_token(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeviceTokenResponse>, AppError> {
    admin.require_super()?;
    let token = new_device_token();
    let token_hash = hash_token(&token);
    let found = state
        .store
        .mutate(|state| {
            let target = state.devices.iter_mut().find(|device| device.id == id);
            target.map(|device| {
                device.token_hash = token_hash;
                device.monitoring = false;
                device.expected_offline = false;
                device.id.clone()
            })
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    Ok(Json(DeviceTokenResponse {
        device_id: found,
        token,
    }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewQuery {
    device_id: Option<String>,
}

async fn overview(
    admin: Admin,
    State(state): State<AppState>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<OverviewResponse>, AppError> {
    let now = Utc::now();
    let snapshot = state.store.snapshot().await;
    if snapshot.devices.is_empty() {
        return Err(AppError::not_found("尚未添加设备"));
    }
    let device = match query.device_id {
        Some(id) => {
            admin.ensure_device_access(&id)?;
            find_device(&snapshot, &id)
                .ok_or_else(|| AppError::not_found("设备不存在"))?
                .clone()
        }
        None => snapshot
            .devices
            .iter()
            .find(|device| admin.can_access_device(&device.id))
            .cloned()
            .ok_or_else(|| AppError::forbidden("尚未分配可管理的设备"))?,
    };
    let from = now - Duration::hours(24);
    let readings = state
        .store
        .read_readings_for(Some(&device.id), from, now)
        .await?;
    let stats = Stats::from_readings(&readings);
    let today = now.with_timezone(&Local).date_naive();
    let device_alerts = state.store.alerts_for(Some(&device.id), 500).await;
    let alert_count_today = device_alerts
        .iter()
        .filter(|alert| {
            matches!(
                alert.kind,
                AlertKind::Noise | AlertKind::Offline | AlertKind::Tamper
            ) && alert.created_at.with_timezone(&Local).date_naive() == today
        })
        .count();
    let alerts = device_alerts.into_iter().take(6).collect();
    let mut settings = crate::models::effective_settings(&snapshot.settings, &device);
    if !admin.is_super() {
        settings.alert_recipients.clear();
        settings.report_recipients.clear();
    }
    let view = device.view(now, settings.offline_timeout_secs);
    Ok(Json(OverviewResponse {
        devices: device_views(&snapshot, now, &admin.user),
        device: view,
        settings,
        stats,
        alerts,
        alert_count_today,
        smtp_configured: state.mailer.configured(),
        server_time: now,
        ice_servers: state.config.ice_servers.clone(),
        signaling_connected: state.hub.device_connected(&device.id).await,
    }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RangeQuery {
    from: Option<String>,
    to: Option<String>,
    max_points: Option<usize>,
    device_id: Option<String>,
}

fn parse_time(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(raw) {
        return Some(value.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return date.and_hms_opt(0, 0, 0).map(|value| value.and_utc());
    }
    if let Ok(value) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Some(value.with_timezone(&Utc));
    }
    None
}

fn parse_range(query: &RangeQuery) -> Result<(DateTime<Utc>, DateTime<Utc>), AppError> {
    let now = Utc::now();
    let to = match &query.to {
        Some(raw) => {
            parse_time(raw).ok_or_else(|| AppError::bad_request("to 时间格式无效，应为 RFC3339"))?
        }
        None => now,
    };
    let from = match &query.from {
        Some(raw) => parse_time(raw)
            .ok_or_else(|| AppError::bad_request("from 时间格式无效，应为 RFC3339"))?,
        None => to - Duration::hours(24),
    };
    if from >= to {
        return Err(AppError::bad_request("开始时间必须早于结束时间"));
    }
    if to - from > Duration::days(90) {
        return Err(AppError::bad_request("单次查询最多支持 90 天"));
    }
    Ok((from, to))
}

async fn list_readings(
    admin: Admin,
    State(state): State<AppState>,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Value>, AppError> {
    let (from, to) = parse_range(&query)?;
    let snapshot = state.store.snapshot().await;
    let device_id = resolve_optional_device(&admin, &snapshot, query.device_id.as_deref())?;
    let readings = state
        .store
        .read_readings_for(device_id.as_deref(), from, to)
        .await?;
    let stats = Stats::from_readings(&readings);
    let max_points = query.max_points.unwrap_or(1800).clamp(50, 5000);
    let readings = downsample(&readings, max_points);
    Ok(Json(json!({
        "from": from,
        "to": to,
        "thresholdDb": device_id
            .as_deref()
            .and_then(|id| find_device(&snapshot, id))
            .map(|device| crate::models::effective_settings(&snapshot.settings, device).threshold_db)
            .unwrap_or(snapshot.settings.threshold_db),
        "readings": readings,
        "stats": stats,
    })))
}

async fn export_readings(
    admin: Admin,
    State(state): State<AppState>,
    Query(query): Query<RangeQuery>,
) -> Result<Response, AppError> {
    let (from, to) = parse_range(&query)?;
    let snapshot = state.store.snapshot().await;
    let device_id = resolve_optional_device(&admin, &snapshot, query.device_id.as_deref())?;
    let readings = state
        .store
        .read_readings_for(device_id.as_deref(), from, to)
        .await?;
    let csv = readings_csv(&readings)?;
    let filename = format!(
        "soundguard-{}-{}.csv",
        from.format("%Y%m%d"),
        to.format("%Y%m%d")
    );
    let mut response = Response::new(Body::from(csv));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| AppError::internal("无法生成下载响应"))?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlertQuery {
    limit: Option<usize>,
    device_id: Option<String>,
}

async fn list_alerts(
    admin: Admin,
    State(state): State<AppState>,
    Query(query): Query<AlertQuery>,
) -> Result<Json<Vec<Alert>>, AppError> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let snapshot = state.store.snapshot().await;
    let device_id = resolve_optional_device(&admin, &snapshot, query.device_id.as_deref())?;
    Ok(Json(
        state.store.alerts_for(device_id.as_deref(), limit).await,
    ))
}

async fn ack_alert(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Alert>, AppError> {
    let existing = state
        .store
        .alert(&id)
        .await
        .ok_or_else(|| AppError::not_found("告警不存在"))?;
    admin.ensure_device_access(&existing.device_id)?;
    let alert = state
        .store
        .acknowledge_alert(&id, true)
        .await?
        .ok_or_else(|| AppError::not_found("告警不存在"))?;
    state.hub.broadcast_alert(&alert);
    Ok(Json(alert))
}

async fn get_evidence(
    admin: Admin,
    State(state): State<AppState>,
    Path(alert_id): Path<String>,
) -> Result<Response, AppError> {
    let alert = state
        .store
        .alert(&alert_id)
        .await
        .ok_or_else(|| AppError::not_found("告警不存在"))?;
    admin.ensure_device_access(&alert.device_id)?;
    let filename = alert
        .evidence_file
        .ok_or_else(|| AppError::not_found("该告警没有录音证据"))?;
    let filename = safe_filename(&filename).map_err(|_| AppError::bad_request("文件名无效"))?;
    let bytes = state
        .store
        .read_evidence(&filename)
        .await?
        .ok_or_else(|| AppError::not_found("证据文件不存在"))?;
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/wav"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename=\"{filename}\""))
            .map_err(|_| AppError::internal("无法生成响应"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

async fn get_settings(
    admin: Admin,
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    Ok(Json(SettingsResponse {
        settings: snapshot.settings,
        smtp_configured: state.mailer.configured(),
        ice_servers: state.config.ice_servers.clone(),
    }))
}

async fn update_settings(
    admin: Admin,
    State(state): State<AppState>,
    Json(mut settings): Json<Settings>,
) -> Result<Json<SettingsResponse>, AppError> {
    admin.require_super()?;
    settings
        .validate_and_normalize()
        .map_err(AppError::bad_request)?;
    let saved = state
        .store
        .mutate(|state| {
            state.settings = settings.clone();
            settings
        })
        .await?;
    state
        .hub
        .broadcast_event("settings", serde_json::to_value(&saved).unwrap_or_default());
    Ok(Json(SettingsResponse {
        settings: saved,
        smtp_configured: state.mailer.configured(),
        ice_servers: state.config.ice_servers.clone(),
    }))
}

async fn control_device(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<ControlRequest>,
) -> Result<Json<DeviceControlResponse>, AppError> {
    admin.ensure_device_access(&request.device_id)?;
    let now = Utc::now();
    let before = state.store.snapshot().await;
    let recipients = crate::models::notification_recipients(
        &before,
        &request.device_id,
        &before.settings.alert_recipients,
    );
    let email_status = initial_email_status(&recipients);
    let mut audit_alert = None;
    let snapshot = state
        .store
        .mutate(|state| {
            let device = state
                .devices
                .iter_mut()
                .find(|device| device.id == request.device_id)?;
            device.desired_monitoring = request.monitoring;
            let mut alert = Alert::new(
                device.id.clone(),
                AlertKind::Control,
                "info",
                if request.monitoring {
                    "管理员恢复采集"
                } else {
                    "管理员暂停采集"
                },
                if request.monitoring {
                    "管理员通过管理端恢复实时声音采集。"
                } else {
                    "管理员通过管理端暂停实时声音采集，客户端保持心跳在线。"
                },
            );
            alert.email_status = email_status;
            state.alerts.insert(0, alert.clone());
            audit_alert = Some(alert);
            Some(state.clone())
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;

    if let Some(alert) = audit_alert {
        state.hub.broadcast_alert(&alert);
        if let Some(device) = find_device(&before, &request.device_id) {
            spawn_simple_security_email(
                state.clone(),
                alert.clone(),
                device.clone(),
                recipients,
                &alert.title,
                &alert.message,
                "#0e7490",
            );
        }
    }
    let _ = state
        .hub
        .send_to_device(
            &request.device_id,
            json!({
                "type": "command",
                "action": "set-monitoring",
                "enabled": request.monitoring
            })
            .to_string(),
        )
        .await;
    let device = find_device(&snapshot, &request.device_id)
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    let settings = crate::models::effective_settings(&snapshot.settings, device);
    let view = device.view(now, settings.offline_timeout_secs);
    state
        .hub
        .broadcast_event("device", serde_json::to_value(&view).unwrap_or_default());
    Ok(Json(DeviceControlResponse {
        device: view,
        settings: if admin.is_super() {
            settings
        } else {
            let mut public = settings;
            public.alert_recipients.clear();
            public.report_recipients.clear();
            public
        },
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceIdRequest {
    device_id: String,
}

async fn send_report(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<DeviceIdRequest>,
) -> Result<Json<ReportResult>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    let recipients = crate::models::notification_recipients(
        &snapshot,
        &request.device_id,
        &snapshot.settings.report_recipients,
    );
    if recipients.is_empty() {
        return Err(AppError::bad_request(
            "请先为用户设置接收邮箱，或在系统设置中配置日报收件人",
        ));
    }
    if find_device(&snapshot, &request.device_id).is_none() {
        return Err(AppError::not_found("设备不存在"));
    }
    let result = send_daily_report(&state, &request.device_id).await?;
    Ok(Json(result))
}

async fn test_alert(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<DeviceIdRequest>,
) -> Result<Json<Alert>, AppError> {
    admin.require_super()?;
    let snapshot = state.store.snapshot().await;
    let device = find_device(&snapshot, &request.device_id)
        .ok_or_else(|| AppError::not_found("设备不存在"))?
        .clone();
    let settings = snapshot.settings.clone();
    let db = round1(settings.threshold_db + 8.0);
    let mut alert = Alert::new(
        device.id.clone(),
        AlertKind::Noise,
        "warning",
        "测试告警 · 分贝超限",
        format!(
            "这是一条由管理员触发的测试告警。模拟平均分贝 {db:.1} dB，阈值 {:.1} dB。",
            settings.threshold_db
        ),
    );
    alert.db = Some(db);
    let recipients =
        crate::models::notification_recipients(&snapshot, &device.id, &settings.alert_recipients);
    alert.email_status = initial_email_status(&recipients);
    state.store.push_alert(alert.clone()).await?;
    state.hub.broadcast_alert(&alert);
    spawn_noise_email(
        state.clone(),
        alert.clone(),
        device,
        settings,
        recipients,
        Vec::new(),
    );
    Ok(Json(alert))
}

fn spawn_noise_email(
    state: AppState,
    alert: Alert,
    device: Device,
    settings: Settings,
    recipients: Vec<String>,
    evidence: Vec<u8>,
) {
    let created = alert
        .created_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let db = alert.db.unwrap_or_default();
    let evidence_note = if alert.evidence_file.is_some() {
        format!(
            "已附带最近 {} 秒现场录音，可在管理端告警中心回放。",
            alert
                .evidence_duration_secs
                .unwrap_or(settings.evidence_seconds)
        )
    } else {
        "本次告警未附带录音。".to_string()
    };
    let subject = format!("【声垣告警】{} 分贝超限 · {:.1} dB", device.name, db);
    let text = format!(
        "声垣 SoundGuard 分贝超限告警\n\n设备：{}\n时间：{}\n平均分贝：{:.1} dB\n阈值：{:.1} dB\n{}\n\n管理端：{}\n",
        device.name,
        created,
        db,
        settings.threshold_db,
        evidence_note,
        state.config.public_base_url
    );
    let body = format!(
        r##"<p style="margin:0 0 18px;color:#cbd5e1;font-size:14px;line-height:1.8">设备 <strong style="color:#fff">{name}</strong> 的 30 秒平均分贝达到 <strong style="color:#fbbf24;font-size:18px">{db:.1} dB</strong>，超过设定阈值 {threshold:.1} dB。</p>
<div style="background:#271c0b;border:1px solid #6b4b16;border-radius:12px;padding:18px;margin:18px 0">
  <div style="color:#fbbf24;font-size:12px;letter-spacing:1px">NOISE ALERT</div>
  <div style="color:#fff;font-size:17px;font-weight:700;margin-top:6px">{time}</div>
  <div style="color:#94a3b8;font-size:12px;margin-top:8px">阈值 {threshold:.1} dB</div>
</div>
<p style="margin:0 0 14px;color:#94a3b8;font-size:13px;line-height:1.8">{note}</p>
<p style="margin:0;color:#94a3b8;font-size:13px;line-height:1.8">请登录管理端查看 24 小时趋势与告警详情。</p>"##,
        name = html_escape(&device.name),
        db = db,
        threshold = settings.threshold_db,
        time = html_escape(&created),
        note = html_escape(&evidence_note),
    );
    let html = email_shell("#92400e", "NOISE THRESHOLD ALERT", "分贝超限告警", &body);
    let attachments = alert_attachment(&alert, evidence);
    spawn_email(
        state,
        alert.id.clone(),
        recipients,
        subject,
        text,
        html,
        attachments,
    );
}

async fn record_pin_failure(
    state: &AppState,
    device: &Device,
    settings: &Settings,
    now: DateTime<Utc>,
    action: &str,
) -> Result<(bool, u32), AppError> {
    let snapshot = state.store.snapshot().await;
    let recipients =
        crate::models::notification_recipients(&snapshot, &device.id, &settings.alert_recipients);
    let email_status = initial_email_status(&recipients);
    let action_label = action.to_string();
    let outcome = state
        .store
        .mutate(|state| {
            let target = state.devices.iter_mut().find(|item| item.id == device.id)?;
            target.stop_failures = target.stop_failures.saturating_add(1);
            let locked = target.stop_failures >= 5;
            if locked {
                target.stop_failures = 0;
                target.stop_locked_until = Some(now + Duration::minutes(5));
            }
            let recent = state.alerts.iter().any(|alert| {
                matches!(alert.kind, AlertKind::Tamper)
                    && alert.device_id == target.id
                    && now.signed_duration_since(alert.created_at).num_seconds() < 60
            });
            let alert = if recent {
                None
            } else {
                let mut alert = Alert::new(
                    target.id.clone(),
                    AlertKind::Tamper,
                    "critical",
                    "设备验证失败",
                    format!(
                        "有人在 {} 尝试验证{}密码但输入错误{}。若非管理员操作，请立即核查现场。",
                        now.with_timezone(&Local).format("%m-%d %H:%M:%S"),
                        action_label,
                        if locked {
                            "，验证入口已锁定 5 分钟"
                        } else {
                            ""
                        }
                    ),
                );
                alert.email_status = email_status;
                state.alerts.insert(0, alert.clone());
                Some(alert)
            };
            let remaining = 5_u32.saturating_sub(target.stop_failures);
            Some((locked, remaining, alert))
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    let (locked, remaining, alert) = outcome;
    if let Some(alert) = alert {
        state.hub.broadcast_alert(&alert);
        spawn_simple_security_email(
            state.clone(),
            alert,
            device.clone(),
            recipients,
            "设备验证失败",
            &format!("有人尝试使用{action}密码验证设备，但密码错误。"),
            "#7f1d1d",
        );
    }
    tokio::time::sleep(StdDuration::from_millis(300)).await;
    if locked {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "验证错误次数过多，入口已锁定 5 分钟",
        ));
    }
    Err(AppError::forbidden(format!(
        "密码错误，还可尝试 {remaining} 次"
    )))
}

fn ensure_pin_unlocked(device: &Device, now: DateTime<Utc>) -> Result<(), AppError> {
    if device
        .stop_locked_until
        .is_some_and(|locked_until| locked_until > now)
    {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "验证尝试次数过多，请 5 分钟后再试",
        ));
    }
    Ok(())
}

async fn reset_pin_failures(state: &AppState, device_id: &str) -> Result<(), AppError> {
    state
        .store
        .mutate(|state| {
            if let Some(device) = state.devices.iter_mut().find(|item| item.id == device_id) {
                device.stop_failures = 0;
                device.stop_locked_until = None;
            }
        })
        .await?;
    Ok(())
}

async fn device_verify_pin(
    _credential: DeviceAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<VerifyPinRequest>,
) -> Result<Json<VerifyPinResponse>, AppError> {
    let device = authenticate_device(&state, &headers, &request.device_id).await?;
    let snapshot = state.store.snapshot().await;
    let now = Utc::now();
    ensure_pin_unlocked(&device, now)?;
    if !secure_eq(&request.pin, &state.config.stop_pin) {
        record_pin_failure(&state, &device, &snapshot.settings, now, "设置").await?;
    }
    reset_pin_failures(&state, &device.id).await?;
    Ok(Json(VerifyPinResponse { verified: true }))
}

async fn device_status(
    admin: Admin,
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    admin.ensure_device_access(&device_id)?;
    let now = Utc::now();
    let snapshot = state.store.snapshot().await;
    let device = find_device(&snapshot, &device_id)
        .ok_or_else(|| AppError::not_found("设备不存在"))?
        .clone();
    let settings = crate::models::effective_settings(&snapshot.settings, &device);
    let alerts = state.store.alerts_for(Some(&device_id), 50).await;
    let latest_noise = alerts
        .iter()
        .find(|alert| matches!(alert.kind, AlertKind::Noise))
        .cloned();
    Ok(Json(json!({
        "device": device.view(now, settings.offline_timeout_secs),
        "thresholdDb": settings.threshold_db,
        "notificationEnabled": settings.notification_enabled,
        "notificationTitle": settings.notification_title,
        "notificationContent": settings.notification_content,
        "latestNoiseAlert": latest_noise,
        "serverTime": now,
    })))
}

async fn device_live(
    _credential: DeviceAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LiveRequest>,
) -> Result<Json<Value>, AppError> {
    authenticate_device(&state, &headers, &request.device_id).await?;
    if !valid_db(request.current_db) {
        return Err(AppError::bad_request("实时分贝数据无效"));
    }
    if request.average_db.is_some_and(|value| !valid_db(value))
        || request.peak_db.is_some_and(|value| !valid_db(value))
    {
        return Err(AppError::bad_request("实时统计数据无效"));
    }
    let recorded_at = request
        .recorded_at
        .filter(|time| Utc::now().signed_duration_since(*time).num_seconds().abs() <= 30)
        .unwrap_or_else(Utc::now);
    let view = state
        .store
        .update_live(&request)
        .await
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    state.hub.broadcast_event(
        "live",
        json!({
            "deviceId": request.device_id,
            "currentDb": view.current_db,
            "averageDb": request.average_db.map(round1),
            "peakDb": request.peak_db.map(round1),
            "capturing": request.capturing,
            "suspended": request.suspended,
            "systemPlaying": request.system_playing,
            "aecEnabled": request.aec_enabled,
            "recordedAt": recorded_at,
            "device": view,
        }),
    );
    Ok(Json(json!({ "accepted": true })))
}

async fn device_heartbeat(
    _credential: DeviceAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<HeartbeatRequest>,
) -> Result<Json<HeartbeatResponse>, AppError> {
    authenticate_device(&state, &headers, &request.device_id).await?;
    let now = Utc::now();
    let result = state
        .store
        .mutate(|state| {
            let device = state
                .devices
                .iter_mut()
                .find(|device| device.id == request.device_id)?;
            let was_offline_alert = device.offline_alert_active;
            device.last_seen = Some(now);
            device.monitoring = request.monitoring;
            device.expected_offline = false;
            device.offline_alert_active = false;
            device.app_version = request.app_version.clone();
            device.capture_error = request
                .capture_error
                .clone()
                .filter(|value| !value.trim().is_empty());
            let recovered = if was_offline_alert {
                let mut alert = Alert::new(
                    device.id.clone(),
                    AlertKind::Recovered,
                    "info",
                    "设备恢复在线",
                    format!(
                        "设备于 {} 恢复心跳，离线告警状态已解除。",
                        now.with_timezone(&Local).format("%m-%d %H:%M:%S")
                    ),
                );
                alert.email_status = "disabled".to_string();
                state.alerts.insert(0, alert.clone());
                Some(alert)
            } else {
                None
            };
            let effective = crate::models::effective_settings(&state.settings, device);
            Some((effective, device.clone(), recovered))
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;

    let (settings, device, recovered) = result;
    if let Some(alert) = recovered {
        info!("Device {} recovered", device.id);
        state.hub.broadcast_alert(&alert);
    }
    let view = device.view(now, settings.offline_timeout_secs);
    state
        .hub
        .broadcast_event("device", serde_json::to_value(&view).unwrap_or_default());
    Ok(Json(HeartbeatResponse {
        desired_monitoring: device.desired_monitoring,
        threshold_db: settings.threshold_db,
        alert_cooldown_secs: settings.alert_cooldown_secs,
        offline_timeout_secs: settings.offline_timeout_secs,
        evidence_seconds: settings.evidence_seconds,
        auto_record_evidence: settings.auto_record_evidence,
        server_time: now,
        ice_servers: state.config.ice_servers.clone(),
        device_id: device.id,
        device_name: device.name,
        notification_enabled: settings.notification_enabled,
        notification_title: settings.notification_title,
        notification_content: settings.notification_content,
        denoise_mode: settings.denoise_mode,
        voice_enhance: settings.voice_enhance,
    }))
}

fn valid_db(value: f64) -> bool {
    value.is_finite() && (0.0..=160.0).contains(&value)
}

fn looks_like_wav(bytes: &[u8]) -> bool {
    crate::wav::duration(bytes).is_some()
}

async fn device_telemetry(
    _credential: DeviceAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TelemetryRequest>,
) -> Result<Json<TelemetryResponse>, AppError> {
    let device = authenticate_device(&state, &headers, &request.device_id).await?;
    if !(1..=300).contains(&request.duration_secs)
        || request.sample_count == 0
        || !valid_db(request.avg_db)
        || !valid_db(request.peak_db)
    {
        return Err(AppError::bad_request("分贝数据超出有效范围"));
    }
    if request.peak_db < request.avg_db
        || request
            .min_db
            .is_some_and(|value| !valid_db(value) || value > request.avg_db)
    {
        return Err(AppError::bad_request("最低分贝数据无效"));
    }

    let now = Utc::now();
    let snapshot = state.store.snapshot().await;
    let settings = crate::models::effective_settings(&snapshot.settings, &device);
    let avg_db = round1(request.avg_db);
    let peak_db = round1(request.peak_db);
    let min_db = round1(request.min_db.unwrap_or(avg_db).min(avg_db));
    let over_threshold = avg_db >= settings.threshold_db;
    let recorded_at = request
        .recorded_at
        .filter(|time| {
            *time <= now + Duration::seconds(30)
                && *time >= now - Duration::days(settings.retention_days as i64)
        })
        .unwrap_or(now);
    let cooldown_elapsed = device
        .last_high_alert_at
        .map(|last| {
            now.signed_duration_since(last).num_seconds() >= settings.alert_cooldown_secs as i64
        })
        .unwrap_or(true);

    let mut evidence_bytes = Vec::new();
    let mut evidence_file = None;
    if over_threshold
        && cooldown_elapsed
        && settings.auto_record_evidence
        && request.audio_base64.is_some()
    {
        match base64::engine::general_purpose::STANDARD
            .decode(request.audio_base64.as_deref().unwrap_or_default())
        {
            Ok(bytes) if bytes.len() <= 6 * 1024 * 1024 && looks_like_wav(&bytes) => {
                match state.store.save_evidence(&bytes, "alert").await {
                    Ok(filename) => {
                        evidence_file = Some(filename);
                        evidence_bytes = bytes;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(_) => warn!("Ignored invalid or oversized evidence audio"),
            Err(error) => warn!("Could not decode evidence audio: {error}"),
        }
    }

    let reading = Reading {
        device_id: request.device_id.clone(),
        recorded_at,
        avg_db,
        peak_db,
        min_db,
        over_threshold,
        duration_secs: request.duration_secs.clamp(1, 300),
        sample_count: request.sample_count.min(60_000_000),
    };
    let new_reading = state.store.append_reading(&reading).await?;

    let recipients = crate::models::notification_recipients(
        &snapshot,
        &request.device_id,
        &settings.alert_recipients,
    );
    let email_status = initial_email_status(&recipients);
    let evidence_duration = crate::wav::duration(&evidence_bytes)
        .map(|s| s.ceil() as u32)
        .unwrap_or(30);
    let mutation = state
        .store
        .mutate(|state| {
            let target = state
                .devices
                .iter_mut()
                .find(|device| device.id == request.device_id)?;
            let was_offline_alert = target.offline_alert_active;
            target.last_seen = Some(now);
            if target
                .last_reading_at
                .is_none_or(|last| recorded_at >= last)
            {
                target.current_db = Some(avg_db);
                target.peak_db = Some(peak_db);
                target.last_reading_at = Some(recorded_at);
            }
            if now.signed_duration_since(recorded_at).num_seconds() <= 60 {
                target.monitoring = target.desired_monitoring;
                target.capture_error = None;
            }
            target.expected_offline = false;
            target.offline_alert_active = false;

            let recovered = if was_offline_alert {
                let mut alert = Alert::new(
                    target.id.clone(),
                    AlertKind::Recovered,
                    "info",
                    "设备恢复在线",
                    format!(
                        "设备于 {} 恢复数据上报，离线告警状态已解除。",
                        now.with_timezone(&Local).format("%m-%d %H:%M:%S")
                    ),
                );
                alert.email_status = "disabled".to_string();
                state.alerts.insert(0, alert.clone());
                Some(alert)
            } else {
                None
            };

            let effective = crate::models::effective_settings(&state.settings, target);
            let cooldown_elapsed = target
                .last_high_alert_at
                .map(|last| {
                    now.signed_duration_since(last).num_seconds()
                        >= effective.alert_cooldown_secs as i64
                })
                .unwrap_or(true);

            let noise_alert = if new_reading && over_threshold && cooldown_elapsed {
                let mut alert = Alert::new(
                    target.id.clone(),
                    AlertKind::Noise,
                    "critical",
                    "30 秒平均分贝超限",
                    format!(
                        "设备 {} 的 30 秒平均分贝为 {avg_db:.1} dB，超过阈值 {:.1} dB。",
                        target.name, effective.threshold_db
                    ),
                );
                alert.db = Some(avg_db);
                alert.evidence_file = evidence_file.clone();
                alert.evidence_duration_secs = evidence_file.as_ref().map(|_| evidence_duration);
                alert.email_status = email_status.clone();
                target.last_high_alert_at = Some(now);
                state.alerts.insert(0, alert.clone());
                Some(alert)
            } else {
                None
            };

            Some((effective, target.clone(), noise_alert, recovered))
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;

    let (latest_settings, device, noise_alert, recovered) = mutation;
    if let Some(alert) = recovered {
        state.hub.broadcast_alert(&alert);
    }
    if let Some(alert) = &noise_alert {
        info!("Noise alert created for {} at {:.1} dB", device.id, avg_db);
        state.hub.broadcast_alert(alert);
        spawn_noise_email(
            state.clone(),
            alert.clone(),
            device.clone(),
            latest_settings.clone(),
            recipients.clone(),
            evidence_bytes,
        );
    }
    let view = device.view(now, latest_settings.offline_timeout_secs);
    state
        .hub
        .broadcast_event("device", serde_json::to_value(&view).unwrap_or_default());

    Ok(Json(TelemetryResponse {
        accepted: true,
        over_threshold,
        threshold_db: latest_settings.threshold_db,
        desired_monitoring: device.desired_monitoring,
        alert_cooldown_secs: latest_settings.alert_cooldown_secs,
        alert: noise_alert,
    }))
}

async fn device_stop_intent(
    _credential: DeviceAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<StopIntentRequest>,
) -> Result<Json<StopIntentResponse>, AppError> {
    let device = authenticate_device(&state, &headers, &request.device_id).await?;
    let now = Utc::now();
    let snapshot = state.store.snapshot().await;
    ensure_pin_unlocked(&device, now)?;
    if !secure_eq(&request.pin, &state.config.stop_pin) {
        record_pin_failure(&state, &device, &snapshot.settings, now, "关闭").await?;
    }

    record_authorized_stop(state, device, false).await
}

async fn device_local_stop(
    _credential: DeviceAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LocalStopRequest>,
) -> Result<Json<StopIntentResponse>, AppError> {
    let device = authenticate_device(&state, &headers, &request.device_id).await?;
    record_authorized_stop(state, device, true).await
}

async fn record_authorized_stop(
    state: AppState,
    device: Device,
    local: bool,
) -> Result<Json<StopIntentResponse>, AppError> {
    let now = Utc::now();
    let snapshot = state.store.snapshot().await;
    let recipients = crate::models::notification_recipients(
        &snapshot,
        &device.id,
        &snapshot.settings.alert_recipients,
    );
    let email_status = initial_email_status(&recipients);
    let title = if local {
        "设备已在本地授权关闭"
    } else {
        "设备已授权关闭"
    };
    let intro = if local {
        "客户端声明已通过本地密码验证并正常关闭。"
    } else {
        "设备已通过服务器关闭密码验证并正常关闭。"
    };
    let alert = state
        .store
        .mutate(|snapshot| {
            let target = snapshot.devices.iter_mut().find(|d| d.id == device.id)?;
            target.expected_offline = true;
            target.monitoring = false;
            target.last_seen = Some(now);
            target.last_stop_at = Some(now);
            target.current_db = None;
            target.live_avg_db = None;
            target.offline_alert_active = false;
            target.stop_failures = 0;
            target.stop_locked_until = None;
            let mut alert = Alert::new(target.id.clone(), AlertKind::Stopped, "info", title, intro);
            alert.email_status = email_status;
            snapshot.alerts.insert(0, alert.clone());
            Some(alert)
        })
        .await?
        .ok_or_else(|| AppError::not_found("设备不存在"))?;
    state.hub.broadcast_alert(&alert);
    if let Some(target) = state.store.device(&device.id).await {
        let settings = crate::models::effective_settings(&snapshot.settings, &target);
        state.hub.broadcast_event(
            "device",
            serde_json::to_value(target.view(now, settings.offline_timeout_secs))?,
        );
    }
    spawn_simple_security_email(state, alert, device, recipients, title, intro, "#0e7490");
    Ok(Json(StopIntentResponse {
        accepted: true,
        message: "关闭授权成功".to_string(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalStopRequest {
    device_id: String,
}

fn spawn_simple_security_email(
    state: AppState,
    alert: Alert,
    device: Device,
    recipients: Vec<String>,
    subject: &str,
    intro: &str,
    accent: &str,
) {
    let created = alert
        .created_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let text = format!(
        "声垣 SoundGuard 安全事件\n\n设备：{}\n时间：{}\n事件：{}\n",
        device.name, created, subject
    );
    let body = format!(
        r##"<p style="margin:0 0 18px;color:#cbd5e1;font-size:14px;line-height:1.8">{intro}</p>
<div style="background:#0b1628;border:1px solid #22304a;border-radius:12px;padding:18px;margin:18px 0">
  <div style="color:#94a3b8;font-size:12px;letter-spacing:1px">EVENT TIME</div>
  <div style="color:#fff;font-size:18px;font-weight:700;margin-top:6px">{time}</div>
  <div style="color:#94a3b8;font-size:12px;margin-top:8px">{device}</div>
</div>
<p style="margin:0;color:#94a3b8;font-size:13px;line-height:1.8">此邮件用于安全审计。如非授权操作，请立即登录管理端核查并联系现场负责人。</p>"##,
        intro = html_escape(intro),
        time = html_escape(&created),
        device = html_escape(&device.name),
    );
    let subject = format!("【声垣安全】{} · {}", device.name, subject);
    let html = email_shell(accent, "SECURITY EVENT", &subject, &body);
    spawn_email(
        state,
        alert.id.clone(),
        recipients,
        subject,
        text,
        html,
        Vec::new(),
    );
}

async fn create_call(
    admin: Admin,
    State(state): State<AppState>,
    Json(request): Json<CreateCallRequest>,
) -> Result<Json<Value>, AppError> {
    admin.ensure_device_access(&request.device_id)?;
    let snapshot = state.store.snapshot().await;
    if find_device(&snapshot, &request.device_id).is_none() {
        return Err(AppError::not_found("设备不存在"));
    }
    let mode = match request.mode.as_str() {
        "intercom" | "broadcast" => request.mode,
        _ => return Err(AppError::bad_request("通话类型无效")),
    };
    if request.ended_at < request.started_at {
        return Err(AppError::bad_request("通话结束时间早于开始时间"));
    }
    let duration_secs = request
        .ended_at
        .signed_duration_since(request.started_at)
        .num_seconds()
        .clamp(0, 86_400) as u32;
    let status = match request.status.as_deref().unwrap_or("completed") {
        "completed" | "missed" | "rejected" | "failed" => {
            request.status.unwrap_or_else(|| "completed".to_string())
        }
        _ => "completed".to_string(),
    };

    let mut audio_file = None;
    let mut audio_mime = None;
    if let Some(raw) = request.audio_base64 {
        let mime = request
            .audio_mime
            .unwrap_or_else(|| "audio/webm".to_string());
        let (mime, extension) = match mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "audio/webm" => ("audio/webm", "webm"),
            "audio/ogg" => ("audio/ogg", "ogg"),
            "audio/wav" | "audio/x-wav" => ("audio/wav", "wav"),
            "audio/mp4" => ("audio/mp4", "mp4"),
            _ => return Err(AppError::bad_request("不支持的录音类型")),
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| AppError::bad_request("录音编码无效"))?;
        if bytes.is_empty() || bytes.len() > 12 * 1024 * 1024 {
            return Err(AppError::bad_request("录音为空或超过 12 MiB，请缩短通话"));
        }
        let valid = match extension {
            "wav" => looks_like_wav(&bytes),
            "webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
            "ogg" => bytes.starts_with(b"OggS"),
            "mp4" => bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
            _ => false,
        };
        if !valid {
            return Err(AppError::bad_request("录音内容与类型不匹配"));
        }
        audio_file = Some(state.store.save_call_audio(&bytes, extension).await?);
        audio_mime = Some(mime.to_string());
    }

    let record = CallRecord {
        id: Uuid::new_v4().to_string(),
        device_id: request.device_id,
        mode,
        started_at: request.started_at,
        ended_at: request.ended_at,
        duration_secs,
        status,
        audio_file,
        audio_mime,
        created_at: Utc::now(),
    };
    let record = state.store.push_call(record).await?;
    let view = record.public_view();
    state.hub.broadcast_event("call-record", view.clone());
    Ok(Json(view))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CallQuery {
    device_id: Option<String>,
    limit: Option<usize>,
}

async fn list_calls(
    admin: Admin,
    State(state): State<AppState>,
    Query(query): Query<CallQuery>,
) -> Result<Json<Vec<Value>>, AppError> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let device_id = query
        .device_id
        .ok_or_else(|| AppError::bad_request("缺少设备编号"))?;
    admin.ensure_device_access(&device_id)?;
    let calls = state.store.calls_for(&device_id, limit).await;
    Ok(Json(calls.iter().map(CallRecord::public_view).collect()))
}

async fn get_call_audio(
    admin: Admin,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let call = state
        .store
        .call(&id)
        .await
        .ok_or_else(|| AppError::not_found("通话记录不存在"))?;
    admin.ensure_device_access(&call.device_id)?;
    serve_call_audio(&state, call).await
}

async fn serve_call_audio(state: &AppState, call: CallRecord) -> Result<Response, AppError> {
    let filename = call
        .audio_file
        .ok_or_else(|| AppError::not_found("该通话没有录音"))?;
    let filename = safe_filename(&filename).map_err(|_| AppError::bad_request("文件名无效"))?;
    let bytes = state
        .store
        .read_call_audio(&filename)
        .await?
        .ok_or_else(|| AppError::not_found("通话录音不存在"))?;
    let mime = match call
        .audio_mime
        .as_deref()
        .unwrap_or("audio/webm")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
    {
        "audio/wav" | "audio/x-wav" => "audio/wav",
        "audio/mp4" => "audio/mp4",
        "audio/ogg" => "audio/ogg",
        _ => "audio/webm",
    };
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("audio/webm")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename=\"{filename}\""))
            .map_err(|_| AppError::internal("无法生成响应"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceCallsQuery {
    device_id: String,
    limit: Option<usize>,
}

async fn device_calls(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DeviceCallsQuery>,
) -> Result<Json<Vec<Value>>, AppError> {
    authenticate_device(&state, &headers, &query.device_id).await?;
    let limit = query.limit.unwrap_or(10).clamp(1, 50);
    let calls = state.store.calls_for(&query.device_id, limit).await;
    Ok(Json(calls.iter().map(CallRecord::public_view).collect()))
}

async fn device_call_audio(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let call = state
        .store
        .call(&id)
        .await
        .ok_or_else(|| AppError::not_found("通话记录不存在"))?;
    authenticate_device(&state, &headers, &call.device_id).await?;
    serve_call_audio(&state, call).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WsQuery {
    token: String,
    device_id: Option<String>,
}

async fn admin_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> Result<Response, AppError> {
    let claims = verify_token(&state.config, &query.token).ok_or_else(AppError::unauthorized)?;
    let user = state
        .store
        .user(&claims.sub)
        .await
        .ok_or_else(AppError::unauthorized)?;
    if user.disabled || user.token_version != claims.ver || user.role.as_str() != claims.role {
        return Err(AppError::unauthorized());
    }
    Ok(ws
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| admin_ws_loop(socket, state, claims)))
}

async fn initial_admin_message(state: &AppState, user: &CurrentUser) -> String {
    let snapshot = state.store.snapshot().await;
    let now = Utc::now();
    let signaling: serde_json::Map<String, Value> = state
        .hub
        .signaling_snapshot()
        .await
        .into_iter()
        .filter(|(device_id, _)| user.can_access_device(device_id))
        .map(|(device_id, connected)| (device_id, Value::Bool(connected)))
        .collect();
    let mut settings = snapshot.settings.clone();
    if !user.is_super() {
        settings.alert_recipients.clear();
        settings.report_recipients.clear();
        settings.daily_report_enabled = false;
        settings.daily_report_time.clear();
    }
    json!({
        "type": "snapshot",
        "data": {
            "devices": device_views(&snapshot, now, user),
            "settings": settings,
            "serverTime": now,
            "signaling": signaling
        }
    })
    .to_string()
}

fn filter_admin_message(message: &str, user: &CurrentUser) -> Option<String> {
    if user.is_super() {
        return Some(message.to_string());
    }
    let mut value: Value = serde_json::from_str(message).ok()?;
    let message_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let allowed = |device_id: Option<&str>| {
        device_id.is_some_and(|id| !id.is_empty() && user.can_access_device(id))
    };
    match message_type {
        "snapshot" => {
            if let Some(data) = value.get_mut("data") {
                if let Some(devices) = data.get_mut("devices").and_then(Value::as_array_mut) {
                    devices.retain(|device| {
                        device
                            .get("id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| user.can_access_device(id))
                    });
                }
                if let Some(settings) = data.get_mut("settings").and_then(Value::as_object_mut) {
                    if let Some(value) = settings.get_mut("alertRecipients") {
                        *value = Value::Array(Vec::new());
                    }
                    if let Some(value) = settings.get_mut("reportRecipients") {
                        *value = Value::Array(Vec::new());
                    }
                    if let Some(value) = settings.get_mut("dailyReportEnabled") {
                        *value = Value::Bool(false);
                    }
                    if let Some(value) = settings.get_mut("dailyReportTime") {
                        *value = Value::String(String::new());
                    }
                }
                if let Some(signaling) = data.get_mut("signaling").and_then(Value::as_object_mut) {
                    signaling.retain(|device_id, _| user.can_access_device(device_id));
                }
            }
        }
        "devices" => {
            if let Some(devices) = value.get_mut("data").and_then(Value::as_array_mut) {
                devices.retain(|device| {
                    device
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| user.can_access_device(id))
                });
            }
        }
        "device" => {
            let device_id = value
                .get("data")
                .and_then(|data| data.get("id"))
                .and_then(Value::as_str);
            if !allowed(device_id) {
                return None;
            }
        }
        "live" | "alert" | "call-record" | "signal" | "hangup" | "signal-error" | "command"
        | "ready" | "signaling" => {
            let device_id = value
                .get("deviceId")
                .and_then(Value::as_str)
                .or_else(|| {
                    value
                        .get("data")
                        .and_then(|data| data.get("deviceId"))
                        .and_then(Value::as_str)
                })
                .or_else(|| {
                    value
                        .get("data")
                        .and_then(|data| data.get("id"))
                        .and_then(Value::as_str)
                });
            if !allowed(device_id) {
                return None;
            }
        }
        "settings" => {
            if let Some(settings) = value.get_mut("data").and_then(Value::as_object_mut) {
                settings.insert("alertRecipients".into(), json!([]));
                settings.insert("reportRecipients".into(), json!([]));
                settings.insert("dailyReportEnabled".into(), json!(false));
                settings.insert("dailyReportTime".into(), json!(""));
            }
        }
        _ => return None,
    }
    Some(value.to_string())
}

async fn ws_user(state: &AppState, claims: &crate::auth::Claims) -> Option<CurrentUser> {
    if Utc::now().timestamp() >= claims.exp as i64 {
        return None;
    }
    let user = state.store.user(&claims.sub).await?;
    if user.disabled || user.token_version != claims.ver || user.role.as_str() != claims.role {
        return None;
    }
    Some(CurrentUser::from_user(&user))
}

async fn admin_ws_loop(socket: WebSocket, state: AppState, claims: crate::auth::Claims) {
    let Some(user) = ws_user(&state, &claims).await else {
        return;
    };
    let connection_id = Uuid::new_v4();
    let mut last_pong = std::time::Instant::now();
    let (mut sender, mut receiver) = socket.split();
    let mut events = state.hub.subscribe_admin();
    let initial = initial_admin_message(&state, &user).await;
    if sender.send(Message::Text(initial.into())).await.is_err() {
        return;
    }
    let mut ticker = tokio::time::interval(StdDuration::from_secs(25));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            broadcast = events.recv() => {
                match broadcast {
                    Ok(message) => {
                        let Some(user) = ws_user(&state, &claims).await else { break; };
                        let target = serde_json::from_str::<Value>(&message).ok()
                            .and_then(|v| v.get("adminConnectionId").and_then(Value::as_str).map(str::to_owned));
                        if target.is_some_and(|target| target != connection_id.to_string()) { continue; }
                        if let Some(filtered) = filter_admin_message(&message, &user) {
                            if sender.send(Message::Text(filtered.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("Admin websocket lagged, skipped {skipped} events");
                        let Some(user) = ws_user(&state, &claims).await else { break; };
                        let resync = initial_admin_message(&state, &user).await;
                        if sender.send(Message::Text(resync.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let Some(user) = ws_user(&state, &claims).await else { break; };
                        let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if !value.is_object() { continue; }
                        let message_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
                        if message_type == "signal" || message_type == "hangup" {
                            let device_id = value.get("deviceId").and_then(Value::as_str).unwrap_or_default().to_string();
                            let call_id = value.get("callId").cloned().unwrap_or_default();
                            if device_id.is_empty() || !user.can_access_device(&device_id) {
                                let error = json!({
                                    "type": "signal-error",
                                    "callId": call_id,
                                    "message": "未指定目标设备或无权访问"
                                });
                                if sender.send(Message::Text(error.to_string().into())).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                            let Some(call_id_text) = call_id.as_str().filter(|id| !id.is_empty() && id.len() <= 128) else { continue; };
                            if !state.hub.claim_call(&device_id, connection_id, call_id_text).await {
                                let error = json!({"type":"signal-error", "callId":call_id, "deviceId":device_id,
                                    "message":"设备正在与另一位管理员通话"});
                                if sender.send(Message::Text(error.to_string().into())).await.is_err() { break; }
                                continue;
                            }
                            let ending = message_type == "hangup";
                            value["from"] = Value::String("admin".to_string());
                            if !state.hub.send_to_device(&device_id, value.to_string()).await {
                                state.hub.finish_call(&device_id, call_id_text).await;
                                let error = json!({
                                    "type": "signal-error",
                                    "callId": call_id,
                                    "deviceId": device_id,
                                    "message": "设备通话通道未连接，请确认客户端在线"
                                });
                                if sender.send(Message::Text(error.to_string().into())).await.is_err() {
                                    break;
                                }
                            }
                            if ending { state.hub.finish_call(&device_id, call_id_text).await; }
                        }
                    }
                    Some(Ok(Message::Pong(_))) => { last_pong = std::time::Instant::now(); }
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(error)) => {
                        warn!("Admin websocket error: {error}");
                        break;
                    }
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                if last_pong.elapsed().as_secs() > 75 || ws_user(&state, &claims).await.is_none() { break; }
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }
    state.hub.disconnect_admin(connection_id).await;
}

async fn device_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> Result<Response, AppError> {
    let device_id = query.device_id.unwrap_or_default();
    authenticate_device_query(&state, &query.token, &device_id).await?;
    Ok(ws
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| device_ws_loop(socket, state, device_id, query.token)))
}

async fn device_ws_loop(socket: WebSocket, state: AppState, device_id: String, token: String) {
    let mut last_pong = std::time::Instant::now();
    let (tx, mut rx) = mpsc::channel::<String>(128);
    let connection_id = state.hub.register_device(device_id.clone(), tx).await;
    state.hub.broadcast_event(
        "signaling",
        json!({ "deviceId": device_id, "connected": true }),
    );
    let (mut sender, mut receiver) = socket.split();
    if sender
        .send(Message::Text(
            json!({ "type": "ready", "deviceId": device_id })
                .to_string()
                .into(),
        ))
        .await
        .is_err()
    {
        state.hub.unregister_device(&device_id, connection_id).await;
        return;
    }
    let mut ticker = tokio::time::interval(StdDuration::from_secs(25));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            outbound = rx.recv() => {
                match outbound {
                    Some(message) => {
                        if authenticate_device_query(&state, &token, &device_id).await.is_err() { break; }
                        if sender.send(Message::Text(message.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if authenticate_device_query(&state, &token, &device_id).await.is_err() { break; }
                        let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if !value.is_object() { continue; }
                        let message_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
                        if message_type == "signal" || message_type == "hangup" {
                            if !state.hub.is_current_device(&device_id, connection_id).await { break; }
                            let call_id = value.get("callId").and_then(Value::as_str).unwrap_or_default().to_string();
                            let Some(owner) = state.hub.call_owner(&device_id, &call_id).await else { continue; };
                            let ending = message_type == "hangup" || value.get("payload").and_then(|p| p.get("kind")).and_then(Value::as_str) == Some("reject");
                            value["adminConnectionId"] = Value::String(owner.to_string());
                            value["from"] = Value::String("device".to_string());
                            value["to"] = Value::String("admin".to_string());
                            value["deviceId"] = Value::String(device_id.clone());
                            state.hub.broadcast_raw(value.to_string());
                            if ending { state.hub.finish_call(&device_id, &call_id).await; }
                        }
                    }
                    Some(Ok(Message::Pong(_))) => { last_pong = std::time::Instant::now(); }
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(error)) => {
                        warn!("Device websocket error: {error}");
                        break;
                    }
                    _ => {}
                }
            }
            _ = ticker.tick() => {
                if last_pong.elapsed().as_secs() > 75 || authenticate_device_query(&state, &token, &device_id).await.is_err() { break; }
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }

    let was_current = state.hub.is_current_device(&device_id, connection_id).await;
    state.hub.unregister_device(&device_id, connection_id).await;
    if was_current {
        state.hub.disconnect_call_device(&device_id).await;
    }
    if !state.hub.device_connected(&device_id).await {
        state.hub.broadcast_event(
            "signaling",
            json!({ "deviceId": device_id, "connected": false }),
        );
    }
}
