mod model;
pub use model::*;

use crate::{anti_addiction_action, data::PlayConfig, get_data, get_data_mut, save_data};
use anyhow::{anyhow, bail, Context, Result};
use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use phire::{judge::{LIMIT_BAD, LIMIT_GOOD}, l10n::LANG_IDENTS, scene::SimpleRecord};
use reqwest::{header, ClientBuilder, Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{borrow::Cow, collections::HashSet, marker::PhantomData, sync::Arc};

pub static CLIENT_TOKEN: Lazy<ArcSwap<Option<String>>> = Lazy::new(|| ArcSwap::from_pointee(None));

static CLIENT: Lazy<ArcSwap<reqwest::Client>> = Lazy::new(|| ArcSwap::from_pointee(basic_client_builder().build().unwrap()));

pub struct Client;

// const API_URL: &str = "http://localhost:2924";
const API_URL: &str = "https://api.phizone.cn";

pub fn basic_client_builder() -> ClientBuilder {
    let mut builder = reqwest::ClientBuilder::new();
    if get_data().accept_invalid_cert {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder
}

fn build_client(access_token: Option<&str>) -> Result<Arc<reqwest::Client>> {
    CLIENT_TOKEN.store(access_token.map(str::to_owned).into());
    let mut headers = header::HeaderMap::new();
    headers.append(header::ACCEPT_LANGUAGE, header::HeaderValue::from_str(&get_data().language.clone().unwrap_or(LANG_IDENTS[0].to_string()))?);
    if let Some(token) = access_token {
        let mut auth_value = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);
    }
    Ok(basic_client_builder().default_headers(headers).build()?.into())
}

pub fn set_access_token_sync(access_token: Option<&str>) -> Result<()> {
    CLIENT.store(build_client(access_token)?);
    Ok(())
}

async fn set_access_token(access_token: &str) -> Result<()> {
    CLIENT.store(build_client(Some(access_token))?);
    Ok(())
}

pub async fn recv_raw(request: RequestBuilder) -> Result<Response> {
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status().as_str().to_owned();
        let text = response.text().await.context("failed to receive text")?;
        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(detail) = what["detail"].as_str() {
                bail!("request failed ({status}): {detail}");
            }
            if let Some(msg) = what["message"].as_str().filter(|m| !m.is_empty()) {
                bail!("request failed ({status}): {msg}");
            }
            if let Some(code) = what["code"].as_str() {
                bail!("request failed ({status}): {code}");
            }
            if let Some(error) = what["error"].as_str() {
                bail!("request failed ({status}): {error}");
            }
            if let Some(desc) = what["error_description"].as_str() {
                bail!("request failed ({status}): {desc}");
            }
        }
        bail!("request failed ({status}): {text}");
    }
    Ok(response)
}

pub enum LoginParams<'a> {
    Password {
        email: &'a str,
        password: &'a str,
    },
    RefreshToken {
        token: &'a str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDto<T> {
    pub status: u32,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    pub data: Option<T>,
    pub total: Option<u64>,
    pub per_page: Option<u64>,
    pub has_previous: Option<bool>,
    pub has_next: Option<bool>,
}

fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => result.push(byte as char),
            b' ' => result.push('+'),
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

impl Client {
    #[inline]
    pub fn get(path: impl AsRef<str>) -> RequestBuilder {
        Self::request(Method::GET, path)
    }

    #[inline]
    pub fn post<T: Serialize>(path: impl AsRef<str>, data: &T) -> RequestBuilder {
        Self::request(Method::POST, path).json(data)
    }

    #[inline]
    pub fn post_form(path: impl AsRef<str>, form: &[(&str, &str)]) -> RequestBuilder {
        let body: String = form
            .iter()
            .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        Self::request(Method::POST, path).header("Content-Type", "application/x-www-form-urlencoded").body(body)
    }

    #[inline]
    pub fn delete(path: impl AsRef<str>) -> RequestBuilder {
        Self::request(Method::DELETE, path)
    }

    pub fn request(method: Method, path: impl AsRef<str>) -> RequestBuilder {
        CLIENT.load().request(method, API_URL.to_string() + path.as_ref())
    }

    pub fn clear_cache<T: Object + 'static>(id: &str) -> Result<bool> {
        let map = obtain_map_cache::<T>();
        let mut guard = map.lock().unwrap();
        let Some(actual_map) = guard.downcast_mut::<ObjectMap::<T>>() else { unreachable!() };
        Ok(actual_map.pop(&id.to_string()).is_some())
    }

    pub async fn load<T: Object + 'static>(id: &str) -> Result<Arc<T>> {
        {
            let map = obtain_map_cache::<T>();
            let mut guard = map.lock().unwrap();
            let Some(actual_map) = guard.downcast_mut::<ObjectMap::<T>>() else { unreachable!() };
            if let Some(value) = actual_map.get(&id.to_string()) {
                return Ok(Arc::clone(value));
            }
            drop(guard);
            drop(map);
        }
        Self::fetch(id).await
    }

    pub async fn fetch<T: Object + 'static>(id: &str) -> Result<Arc<T>> {
        Self::fetch_opt(id).await?.ok_or_else(|| anyhow!("entry not found"))
    }

    pub async fn fetch_opt<T: Object + 'static>(id: &str) -> Result<Option<Arc<T>>> {
        let value = Client::fetch_inner::<T>(id).await?;
        let Some(value) = value else { return Ok(None) };
        let value = Arc::new(value);
        let map = obtain_map_cache::<T>();
        let mut guard = map.lock().unwrap();
        let Some(actual_map) = guard.downcast_mut::<ObjectMap::<T>>() else {
            unreachable!()
        };
        actual_map.put(id.to_string(), Arc::clone(&value));
        Ok(Some(value))
    }

    async fn fetch_inner<T: Object>(id: &str) -> Result<Option<T>> {
        let resp = Self::get(format!("/{}/{id}", T::QUERY_PATH)).send().await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_str().to_owned();
            let text = resp.text().await.context("failed to receive text")?;
            if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(detail) = what["detail"].as_str() {
                    bail!("request failed ({status}): {detail}");
                }
                if let Some(msg) = what["message"].as_str().filter(|m| !m.is_empty()) {
                    bail!("request failed ({status}): {msg}");
                }
                if let Some(code) = what["code"].as_str() {
                    bail!("request failed ({status}): {code}");
                }
            }
            bail!("request failed ({status}): {text}");
        }
        let resp: ResponseDto<T> = resp.json().await?;
        Ok(resp.data)
    }

    pub fn query<T: Object>() -> QueryBuilder<T> {
        QueryBuilder {
            queries: Vec::new(),
            page: None,
            suffix: "",
            _phantom: PhantomData::default(),
        }
    }

    pub async fn register(email: &str, username: &str, password: &str) -> Result<()> {
        recv_raw(Self::post(
            "/users/brief",
            &json!({
                "email": email,
                "userName": username,
                "password": password,
            }),
        ))
        .await?;
        Ok(())
    }

    pub async fn login(params: LoginParams<'_>) -> Result<()> {
        #[derive(Deserialize)]
        struct Resp {
            access_token: String,
            token_type: String,
            expires_in: u32,
            scope: String,
            refresh_token: String,
        }
        let mut form: Vec<(&str, &str)> = vec![("client_id", "public")];
        match &params {
            LoginParams::Password { email, password } => {
                form.push(("grant_type", "password"));
                form.push(("username", email));
                form.push(("password", password));
            }
            LoginParams::RefreshToken { token } => {
                form.push(("grant_type", "refresh_token"));
                form.push(("refresh_token", token));
            }
        }
        let resp: Resp = recv_raw(Self::post_form("/auth/token", &form)).await?.json().await?;

        anti_addiction_action("startup", Some("PhiZone".to_string()));

        set_access_token(&resp.access_token).await?;
        get_data_mut().tokens = Some((resp.access_token, resp.refresh_token));
        save_data()?;
        Ok(())
    }

    pub async fn get_me() -> Result<User> {
        let resp: ResponseDto<User> = recv_raw(Self::get("/me")).await?.json().await?;
        resp.data.ok_or_else(|| anyhow!("no user data in response"))
    }

    pub async fn best_record(_id: i32) -> Result<SimpleRecord> {
        bail!("best_record not yet implemented for new API")
    }

    pub async fn upload_file(_name: &str, _bytes: Vec<u8>) -> Result<String> {
        bail!("upload_file not yet implemented for new API")
    }

    pub async fn create_play_configuration(pc: &PlayConfig) -> Result<String> {
        #[derive(Deserialize)]
        struct Created {
            id: String,
        }
        let resp: ResponseDto<Created> = recv_raw(Self::post("/player/configurations", &play_configuration_body(pc)))
            .await?
            .json()
            .await?;
        resp.data.map(|it| it.id).ok_or_else(|| anyhow!("no configuration id in response"))
    }

    pub async fn update_play_configuration(id: &str, pc: &PlayConfig) -> Result<()> {
        let patch = serde_json::json!([
            { "op": "replace", "path": "/name", "value": pc.name.clone() },
            { "op": "replace", "path": "/perfectJudgment", "value": judgment_ms(pc.perfect_judgment) },
            { "op": "replace", "path": "/goodJudgment", "value": judgment_ms(pc.good_judgment) },
        ]);
        recv_raw(Self::request(Method::PATCH, format!("/player/configurations/{id}")).json(&patch)).await?;
        Ok(())
    }

    pub async fn delete_play_configuration(id: &str) -> Result<()> {
        recv_raw(Self::delete(format!("/player/configurations/{id}"))).await?;
        Ok(())
    }
}

fn judgment_ms(seconds: f64) -> i32 {
    (seconds * 1000.).round() as i32
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayConfigurationDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub perfect_judgment: i32,
    #[serde(default)]
    pub good_judgment: i32,
    #[serde(default)]
    pub chart_mirroring: i32,
    #[serde(default)]
    pub aspect_ratio: Option<Vec<i32>>,
    #[serde(default)]
    pub note_size: f64,
    #[serde(default)]
    pub background_luminance: f64,
    #[serde(default)]
    pub background_blur: f64,
    #[serde(default)]
    pub simultaneous_note_hint: bool,
    #[serde(default)]
    pub fc_ap_indicator: bool,
    #[serde(default)]
    pub chart_offset: i32,
    #[serde(default)]
    pub hit_sound_volume: f64,
    #[serde(default)]
    pub music_volume: f64,
}

fn play_configuration_body(pc: &PlayConfig) -> PlayConfigurationDto {
    PlayConfigurationDto {
        id: None,
        name: Some(pc.name.clone()),
        perfect_judgment: judgment_ms(pc.perfect_judgment),
        good_judgment: judgment_ms(pc.good_judgment),
        chart_mirroring: 0,
        aspect_ratio: None,
        note_size: 1.0,
        background_luminance: 0.5,
        background_blur: 1.0,
        simultaneous_note_hint: true,
        fc_ap_indicator: true,
        chart_offset: 0,
        hit_sound_volume: 1.0,
        music_volume: 1.0,
    }
}

/// Creates the play configuration on the server if it has no id yet, otherwise
/// patches the server with the current local values. Returns the configuration id.
/// Must run before `start_play`, since the server computes rksFactor from the
/// stored configuration when the record is submitted.
pub async fn sync_active_play_config() -> Result<Option<String>> {
    let Some(pc) = get_data().active_play_config() else {
        return Ok(None);
    };
    let pc = pc.clone();
    let id = match &pc.id {
        Some(id) => {
            Client::update_play_configuration(id, &pc).await?;
            Some(id.clone())
        }
        None => Some(Client::create_play_configuration(&pc).await?),
    };
    if let Some(config) = get_data_mut().active_play_config_mut() {
        config.id = id.clone();
    }
    save_data()?;
    Ok(id)
}

/// Downloads the user's play configurations from the server and merges them
/// into the local list (updates by id, appends new ones; configs missing on
/// the server keep their values but become local-only by clearing the id).
pub async fn download_play_configurations() -> Result<()> {
    let resp: ResponseDto<Vec<PlayConfigurationDto>> =
        recv_raw(Client::get("/player/configurations").query(&[("page", "1"), ("perPage", "100")]))
            .await?
            .json()
            .await?;
    let list = resp.data.unwrap_or_default();
    let data = get_data_mut();
    let ids: HashSet<&str> = list.iter().filter_map(|it| it.id.as_deref()).collect();
    for local in &mut data.play_configs {
        if local.id.as_deref().is_some_and(|id| !ids.contains(id)) {
            local.id = None;
        }
    }
    for server in list {
        let Some(id) = server.id else { continue };
        let name = server.name.clone().unwrap_or_default();
        let (perfect, good) = (server.perfect_judgment as f64 / 1000., server.good_judgment as f64 / 1000.);
        let bad = good * LIMIT_BAD / LIMIT_GOOD;
        if let Some(local) = data.play_configs.iter_mut().find(|it| it.id.as_deref() == Some(id.as_str())) {
            if !name.is_empty() {
                local.name = name;
            }
            if (local.perfect_judgment - perfect).abs() > 1e-9 || (local.good_judgment - good).abs() > 1e-9 {
                local.bad_judgment = good * LIMIT_BAD / LIMIT_GOOD;
            }
            local.perfect_judgment = perfect;
            local.good_judgment = good;
        } else if let Some(local) = data.play_configs.iter_mut().find(|it| it.id.is_none() && it.name == name) {
            local.id = Some(id);
            if (local.perfect_judgment - perfect).abs() > 1e-9 || (local.good_judgment - good).abs() > 1e-9 {
                local.bad_judgment = good * LIMIT_BAD / LIMIT_GOOD;
            }
            local.perfect_judgment = perfect;
            local.good_judgment = good;
        } else {
            data.play_configs.push(PlayConfig {
                id: Some(id),
                name,
                perfect_judgment: perfect,
                good_judgment: good,
                bad_judgment: bad,
            });
        }
    }
    if data.play_configs.is_empty() {
        data.play_configs.push(PlayConfig::default());
    }
    if let Some(index) = data.active_play_config {
        if index >= data.play_configs.len() {
            data.active_play_config = Some(0);
        }
    } else {
        data.active_play_config = Some(0);
    }
    save_data()?;
    Ok(())
}

#[must_use]
pub struct QueryBuilder<T> {
    queries: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    page: Option<u64>,
    suffix: &'static str,
    _phantom: PhantomData<T>,
}

impl<T: Object> QueryBuilder<T> {
    pub fn query(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        self.queries.push((key.into(), value.into()));
        self
    }

    #[inline]
    pub fn order(self, order: impl Into<Cow<'static, str>>) -> Self {
        self.query("order", order)
    }

    #[inline]
    pub fn tags(mut self, include: Vec<String>, exclude: Vec<String>) -> Self {
        for tag in include {
            self.queries.push(("tagsToInclude".into(), tag.into()));
        }
        for tag in exclude {
            self.queries.push(("tagsToExclude".into(), tag.into()));
        }
        self
    }

    #[inline]
    pub fn search(self, search: impl Into<Cow<'static, str>>) -> Self {
        self.query("search", search)
    }

    #[inline]
    pub fn page_num(self, page_num: u64) -> Self {
        self.query("perPage", page_num.to_string())
    }

    #[inline]
    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn page(mut self, page: u64) -> Self {
        self.page = Some(page);
        self
    }

    pub async fn send(mut self) -> Result<(Vec<T>, u64)> {
        self.queries.push(("page".into(), (self.page.unwrap_or(0) + 1).to_string().into()));
        let res: ResponseDto<Vec<T>> =
            recv_raw(Client::get(format!("/{}{}", T::QUERY_PATH, self.suffix)).query(&self.queries))
                .await?
                .json()
                .await?;
        Ok((res.data.unwrap_or_default(), res.total.unwrap_or(0)))
    }
}
