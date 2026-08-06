use std::time::Duration;

use crate::models::DownloadSettings;

pub fn validate_download_settings(settings: &DownloadSettings) -> Result<(), String> {
    if !settings.proxy_enabled {
        return Ok(());
    }
    let value = settings.proxy_url.trim();
    if value.is_empty() {
        return Err("启用下载代理后必须填写代理地址".to_string());
    }
    let url = reqwest::Url::parse(value).map_err(|error| format!("代理地址无效：{error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("下载代理目前仅支持 http:// 或 https:// 地址".to_string());
    }
    if url.host_str().is_none() {
        return Err("代理地址缺少主机名".to_string());
    }
    Ok(())
}

pub fn download_client(
    settings: &DownloadSettings,
    connect_timeout: Duration,
) -> Result<reqwest::Client, String> {
    validate_download_settings(settings)?;
    // Keep download networking isolated from process/system proxy discovery.
    // The application uses a proxy only when the user explicitly enables it.
    let mut builder = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .no_proxy();
    if settings.proxy_enabled {
        let proxy = reqwest::Proxy::all(settings.proxy_url.trim())
            .map_err(|error| format!("无法配置下载代理：{error}"))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|error| error.to_string())
}

pub async fn test_download_proxy(proxy_url: &str) -> Result<String, String> {
    let settings = DownloadSettings {
        proxy_enabled: true,
        proxy_url: proxy_url.trim().to_string(),
    };
    let client = download_client(&settings, Duration::from_secs(10))?;
    let pypi = tokio::time::timeout(
        Duration::from_secs(15),
        client
            .get("https://pypi.org/pypi/nvidia-cudnn-cu12/9.10.2.21/json")
            .send(),
    )
    .await
    .map_err(|_| "通过代理连接 PyPI 超时".to_string())?
    .map_err(|error| format!("通过代理连接 PyPI 失败：{error}"))?;
    if !pypi.status().is_success() {
        return Err(format!("代理访问 PyPI 返回 HTTP {}", pypi.status()));
    }
    let hugging_face = tokio::time::timeout(
        Duration::from_secs(15),
        client.get("https://huggingface.co/").send(),
    )
    .await
    .map_err(|_| "通过代理连接 Hugging Face 超时".to_string())?
    .map_err(|error| format!("通过代理连接 Hugging Face 失败：{error}"))?;
    if !hugging_face.status().is_success() {
        return Err(format!(
            "代理访问 Hugging Face 返回 HTTP {}",
            hugging_face.status()
        ));
    }
    Ok("代理可访问 PyPI 与 Hugging Face".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_proxy_and_rejects_unsupported_schemes() {
        assert!(validate_download_settings(&DownloadSettings {
            proxy_enabled: true,
            proxy_url: "http://127.0.0.1:10808".to_string(),
        })
        .is_ok());
        assert!(validate_download_settings(&DownloadSettings {
            proxy_enabled: true,
            proxy_url: "socks5://127.0.0.1:10808".to_string(),
        })
        .is_err());
    }

    #[test]
    fn disabled_proxy_does_not_require_an_address() {
        assert!(validate_download_settings(&DownloadSettings::default()).is_ok());
    }
}
