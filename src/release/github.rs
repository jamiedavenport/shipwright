use super::{
    Context, Result,
    archive::{checksum, hash_reader},
    check_cancel, err,
    git::git,
    http::encode,
};
use serde_json::{Value, json};
use std::{fs::File, path::Path};

pub(super) struct Github {
    repository: String,
    token: Option<String>,
}
impl Github {
    pub(super) fn new(cx: &Context<'_>, dry_run: bool) -> Result<Self> {
        let origin = git(cx, &["remote", "get-url", "origin"])?;
        let repository = origin
            .strip_prefix("https://github.com/")
            .or_else(|| origin.strip_prefix("git@github.com:"))
            .or_else(|| origin.strip_prefix("ssh://git@github.com/"))
            .ok_or("Binary releases require a github.com origin")?
            .trim_end_matches('/')
            .trim_end_matches(".git");
        let parts = repository.split('/').collect::<Vec<_>>();
        if parts.len() != 2
            || parts.iter().any(|s| {
                s.is_empty()
                    || *s == "."
                    || *s == ".."
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        {
            return Err("Unsupported GitHub origin".into());
        }
        let token = std::env::var("GH_TOKEN")
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|v| !v.is_empty()));
        if token.is_none() && !dry_run {
            return Err("GitHub binary releases need GH_TOKEN or GITHUB_TOKEN with repository contents write access".into());
        }
        Ok(Self {
            repository: repository.into(),
            token,
        })
    }
    pub(super) fn request(
        &self,
        cx: &Context<'_>,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value)> {
        check_cancel(cx)?;
        let url = format!("https://api.github.com/repos/{}/{}", self.repository, path);
        let authorization = self
            .token
            .as_ref()
            .map(|s| format!("Bearer {s}"))
            .unwrap_or_default();
        let mut response = match method {
            "POST" => cx
                .http
                .post(&url)
                .header("Authorization", &authorization)
                .header("User-Agent", "shipwright")
                .header("Accept", "application/vnd.github+json")
                .send_json(body.unwrap_or(json!({}))),
            "PATCH" => cx
                .http
                .patch(&url)
                .header("Authorization", &authorization)
                .header("User-Agent", "shipwright")
                .header("Accept", "application/vnd.github+json")
                .send_json(body.unwrap_or(json!({}))),
            _ => {
                let mut request = cx
                    .http
                    .get(&url)
                    .header("User-Agent", "shipwright")
                    .header("Accept", "application/vnd.github+json");
                if self.token.is_some() {
                    request = request.header("Authorization", &authorization);
                }
                request.call()
            }
        }
        .map_err(|_| "GitHub request failed; check connectivity and credentials")?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) && status != 404 {
            return Err(format!("GitHub request failed (HTTP {status})"));
        }
        let value = response
            .body_mut()
            .read_json()
            .map_err(|_| "Invalid GitHub response")?;
        Ok((status, value))
    }
    pub(super) fn release(&self, cx: &Context<'_>, tag: &str) -> Result<Value> {
        let (status, release) =
            self.request(cx, "GET", &format!("releases/tags/{}", encode(tag)), None)?;
        if status == 200 {
            return Ok(release);
        }
        let (status, release) = self.request(
            cx,
            "POST",
            "releases",
            Some(json!({"tag_name":tag,"target_commitish":cx.commit,"name":tag,"draft":true})),
        )?;
        if status != 201 {
            return Err(format!("Could not create GitHub draft (HTTP {status})"));
        }
        Ok(release)
    }
    pub(super) fn upload(
        &self,
        cx: &Context<'_>,
        release: &Value,
        path: &Path,
        dry_run: bool,
    ) -> Result<()> {
        check_cancel(cx)?;
        let id = release["id"].as_u64().ok_or("GitHub release has no ID")?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid archive name")?;
        let digest = checksum(path)?;
        let mut page = 1;
        loop {
            let (status, assets) = self.request(
                cx,
                "GET",
                &format!("releases/{id}/assets?per_page=100&page={page}"),
                None,
            )?;
            if status != 200 {
                return Err("Could not list GitHub assets".into());
            }
            let assets = assets.as_array().ok_or("Invalid GitHub asset list")?;
            if let Some(asset) = assets.iter().find(|a| a["name"] == name) {
                let remote = if let Some(digest) = asset["digest"].as_str() {
                    digest.to_owned()
                } else {
                    self.asset_checksum(cx, asset)?
                };
                if asset["state"] != "uploaded" || remote != digest {
                    return Err(format!(
                        "GitHub asset {name} conflicts with local contents; refusing to overwrite"
                    ));
                }
                eprintln!("{name}: already uploaded (matching SHA-256)");
                return Ok(());
            }
            if assets.len() < 100 {
                break;
            }
            page += 1;
        }
        if dry_run {
            return Ok(());
        }
        let url = format!(
            "https://uploads.github.com/repos/{}/releases/{id}/assets?name={}",
            self.repository,
            encode(name)
        );
        let response = cx
            .http
            .post(&url)
            .header("User-Agent", "shipwright")
            .header(
                "Authorization",
                &format!("Bearer {}", self.token.as_deref().unwrap_or_default()),
            )
            .header("Content-Type", "application/gzip")
            .send(File::open(path).map_err(err)?)
            .map_err(|_| "GitHub asset upload failed")?;
        if response.status().as_u16() != 201 {
            return Err(format!(
                "GitHub asset upload failed (HTTP {})",
                response.status()
            ));
        }
        eprintln!("{name}: uploaded");
        Ok(())
    }
    fn asset_checksum(&self, cx: &Context<'_>, asset: &Value) -> Result<String> {
        let id = asset["id"].as_u64().ok_or("GitHub asset has no ID")?;
        let url = format!(
            "https://api.github.com/repos/{}/releases/assets/{id}",
            self.repository
        );
        let mut request = cx
            .http
            .get(&url)
            .header("User-Agent", "shipwright")
            .header("Accept", "application/octet-stream");
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = request
            .call()
            .map_err(|_| "Could not download existing GitHub asset")?;
        if response.status().as_u16() == 302 {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("Missing GitHub asset redirect")?;
            if !location.starts_with("https://") {
                return Err("Unsafe GitHub asset redirect".into());
            }
            // Signed asset URLs need no Authorization header.
            response = cx
                .http
                .get(location)
                .call()
                .map_err(|_| "Could not download existing GitHub asset")?;
        }
        if response.status().as_u16() != 200 {
            return Err(format!(
                "GitHub asset download failed (HTTP {})",
                response.status()
            ));
        }
        hash_reader(response.body_mut().as_reader())
            .map_err(|_| "Could not checksum GitHub asset".into())
    }
    pub(super) fn finish(&self, cx: &Context<'_>, release: &Value) -> Result<()> {
        if release["draft"] == true {
            let id = release["id"].as_u64().ok_or("GitHub release has no ID")?;
            let (status, _) = self.request(
                cx,
                "PATCH",
                &format!("releases/{id}"),
                Some(json!({"draft":false})),
            )?;
            if status != 200 {
                return Err("Could not publish GitHub draft".into());
            }
        }
        Ok(())
    }
}
