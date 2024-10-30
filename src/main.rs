mod gui;

use anyhow::{anyhow, Result};
use reqwest::{Client, header};
use serde_json::Value;
use std::{io::Write, time::Duration, collections::HashMap};
use tokio::{fs::File, io::AsyncWriteExt};
use url::Url;
use base64::Engine;
use aes::Aes128;
use cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use cbc::Decryptor;

// Update the main function to support both CLI and GUI modes:
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.len() {
        1 => {
            // No arguments, run GUI mode
            if let Err(e) = gui::run_gui() {
                eprintln!("Failed to run GUI: {}", e);
            }
            Ok(())
        }
        2 => {
            // One argument (URL), run CLI mode
            let embed_url = &args[1];
            let downloader = HotmartDownloader::new()?;
            downloader.download_video(embed_url).await
        }
        _ => {
            println!("Usage:");
            println!("  {} [url]   - Download video from URL (CLI mode)", args[0]);
            println!("  {}         - Launch GUI", args[0]);
            Ok(())
        }
    }
}

struct HotmartDownloader {
    client: Client,
}

impl HotmartDownloader {

    fn new() -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert("User-Agent", header::HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0"));
        headers.insert("Accept", header::HeaderValue::from_static("*/*"));
        headers.insert("Accept-Language", header::HeaderValue::from_static("en-US,en;q=0.5"));
        headers.insert("Origin", header::HeaderValue::from_static("https://player.hotmart.com"));
        headers.insert("Referer", header::HeaderValue::from_static("https://player.hotmart.com/"));
        headers.insert("Connection", header::HeaderValue::from_static("keep-alive"));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self { client })
    }

    pub async fn download_video_with_progress_and_path<F, P>(
        &self,
        embed_url: &str,
        save_path: P,
        progress_callback: F
    ) -> Result<()>
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
        P: AsRef<std::path::Path>,
    {
        let url = Url::parse(embed_url)?;
        let video_id = url.path_segments()
            .ok_or_else(|| anyhow!("Invalid URL path"))?
            .last()
            .ok_or_else(|| anyhow!("No video ID found"))?;

        let token = url.query_pairs()
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        let signature = url.query_pairs()
            .find(|(key, _)| key == "signature")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        let master_playlist_url = match self.get_master_playlist_url(embed_url).await {
            Ok(url) => url,
            Err(e) => {
                progress_callback(0, 100);
                self.get_api_playlist_url(video_id, &token, &signature).await?
            }
        };

        let best_quality_url = self.get_best_quality_stream(&master_playlist_url).await?;
        let media_segments = self.get_media_segments(&best_quality_url).await?;
        let total_segments = media_segments.len();

        let mut output_file = File::create(save_path).await?;
        let mut key_cache: HashMap<String, Vec<u8>> = HashMap::new();

        for (i, (segment_url, encryption_info)) in media_segments.iter().enumerate() {
            progress_callback(i + 1, total_segments);

            let mut segment_data = self.client.get(segment_url)
                .send()
                .await?
                .bytes()
                .await?
                .to_vec();

            if let Some((key_url, iv)) = encryption_info {
                let decryption_key = if let Some(cached_key) = key_cache.get(key_url) {
                    cached_key.clone()
                } else {
                    let key_data = self.client.get(key_url)
                        .send()
                        .await?
                        .bytes()
                        .await?
                        .to_vec();
                    key_cache.insert(key_url.clone(), key_data.clone());
                    key_data
                };

                segment_data = self.decrypt_segment(&segment_data, &decryption_key, &iv).await?;
            }

            output_file.write_all(&segment_data).await?;
        }

        Ok(())
    }

    // New method with progress callback
    pub async fn download_video_with_progress<F>(&self, embed_url: &str, progress_callback: F) -> Result<()>
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        let url = Url::parse(embed_url)?;
        let video_id = url.path_segments()
            .ok_or_else(|| anyhow!("Invalid URL path"))?
            .last()
            .ok_or_else(|| anyhow!("No video ID found"))?;

        // Get both token and signature from URL
        let token = url.query_pairs()
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        let signature = url.query_pairs()
            .find(|(key, _)| key == "signature")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        // Fetch master playlist URL either directly or via API
        let master_playlist_url = match self.get_master_playlist_url(embed_url).await {
            Ok(url) => url,
            Err(e) => {
                progress_callback(0, 100); // Initial progress indication
                self.get_api_playlist_url(video_id, &token, &signature).await?
            }
        };

        // Get best quality stream URL
        let best_quality_url = self.get_best_quality_stream(&master_playlist_url).await?;

        // Fetch list of segments
        let media_segments = self.get_media_segments(&best_quality_url).await?;
        let total_segments = media_segments.len();

        let output_path = format!("{}.mp4", video_id);
        let mut output_file = File::create(&output_path).await?;

        // Cache for decryption keys
        let mut key_cache: HashMap<String, Vec<u8>> = HashMap::new();

        for (i, (segment_url, encryption_info)) in media_segments.iter().enumerate() {
            // Update progress
            progress_callback(i + 1, total_segments);

            let mut segment_data = self.client.get(segment_url)
                .send()
                .await?
                .bytes()
                .await?
                .to_vec();

            // If segment is encrypted, decrypt it
            if let Some((key_url, iv)) = encryption_info {
                let decryption_key = if let Some(cached_key) = key_cache.get(key_url) {
                    cached_key.clone()
                } else {
                    let key_data = self.client.get(key_url)
                        .send()
                        .await?
                        .bytes()
                        .await?
                        .to_vec();
                    key_cache.insert(key_url.clone(), key_data.clone());
                    key_data
                };

                segment_data = self.decrypt_segment(&segment_data, &decryption_key, &iv).await?;
            }

            output_file.write_all(&segment_data).await?;
        }

        // Final progress update
        progress_callback(total_segments, total_segments);

        Ok(())
    }

    async fn download_video(&self, embed_url: &str) -> Result<()> {
        let url = Url::parse(embed_url)?;
        let video_id = url.path_segments()
            .ok_or_else(|| anyhow!("Invalid URL path"))?
            .last()
            .ok_or_else(|| anyhow!("No video ID found"))?;

        println!("Extracting video info for ID: {}", video_id);

        // Get both token and signature from URL
        let token = url.query_pairs()
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        let signature = url.query_pairs()
            .find(|(key, _)| key == "signature")
            .map(|(_, value)| value.to_string())
            .unwrap_or_default();

        // Fetch master playlist URL either directly or via API
        let master_playlist_url = match self.get_master_playlist_url(embed_url).await {
            Ok(url) => url,
            Err(e) => {
                println!("Page parsing failed: {}. Trying API...", e);
                self.get_api_playlist_url(video_id, &token, &signature).await?
            }
        };

        println!("Found master playlist: {}", master_playlist_url);

        // Get best quality stream URL
        let best_quality_url = self.get_best_quality_stream(&master_playlist_url).await?;
        println!("Selected best quality stream: {}", best_quality_url);

        // Fetch list of segments
        let media_segments = self.get_media_segments(&best_quality_url).await?;
        println!("Found {} segments to download", media_segments.len());

        let output_path = format!("{}.mp4", video_id);
        let mut output_file = File::create(&output_path).await?;

        // Cache for decryption keys
        let mut key_cache: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();

        for (i, (segment_url, encryption_info)) in media_segments.iter().enumerate() {
            print!("\rDownloading segment {}/{}", i + 1, media_segments.len());
            let _ = std::io::stdout().flush();

            let mut segment_data = self.client.get(segment_url)
                .send()
                .await?
                .bytes()
                .await?
                .to_vec();

            // If segment is encrypted, decrypt it
            if let Some((key_url, iv)) = encryption_info {
                let decryption_key = if let Some(cached_key) = key_cache.get(key_url) {
                    cached_key.clone()
                } else {
                    let key_data = self.client.get(key_url)
                        .send()
                        .await?
                        .bytes()
                        .await?
                        .to_vec();
                    key_cache.insert(key_url.clone(), key_data.clone());
                    key_data
                };

                segment_data = self.decrypt_segment(&segment_data, &decryption_key, &iv).await?;
            }

            output_file.write_all(&segment_data).await?;
        }

        println!("\nDownload complete: {}", output_path);
        Ok(())
    }

    async fn fetch_and_decrypt_segment(&self, segment_url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(segment_url).send().await?.text().await?;
        let decoded_data = base64::engine::general_purpose::STANDARD
            .decode(response.trim())
            .map_err(|e| anyhow!("Failed to decode segment: {}", e))?;

        // Check for encryption
        if let Some((key_url, iv)) = self.get_encryption_details(segment_url).await? {
            let key = self.fetch_decryption_key(&key_url).await?;
            let decrypted_data = self.decrypt_segment(&decoded_data, &key, &iv).await?;  // Add await here
            Ok(decrypted_data)
        } else {
            // If not encrypted, return the decoded data directly
            Ok(decoded_data)
        }
    }

    async fn get_encryption_details(&self, _segment_url: &str) -> Result<Option<(String, Vec<u8>)>> {
        // Placeholder function to fetch key_url and IV from `.m3u8` playlist or segment headers
        // Implement actual retrieval logic based on playlist format if applicable
        Ok(None)
    }

    async fn fetch_decryption_key(&self, key_url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(key_url).send().await?.text().await?;
        let key = base64::engine::general_purpose::STANDARD
            .decode(response.trim())
            .map_err(|e| anyhow!("Failed to decode key: {}", e))?;
        Ok(key)
    }

    async fn decrypt_segment(&self, data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
        let decryptor = Decryptor::<Aes128>::new_from_slices(key, iv)
            .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

        let mut buf = data.to_vec();
        let decrypted_data = decryptor.decrypt_padded_vec_mut::<Pkcs7>(&mut buf)
            .map_err(|e| anyhow!("Failed to decrypt: {}", e))?;

        Ok(decrypted_data.to_vec())
    }

    async fn get_master_playlist_url(&self, embed_url: &str) -> Result<String> {
        let response = self.client.get(embed_url).send().await?;
        let page_html = response.text().await?;

        if let Some(start) = page_html.find(r#"<script id="__NEXT_DATA__" type="application/json">"#) {
            let json_start = start + r#"<script id="__NEXT_DATA__" type="application/json">"#.len();
            if let Some(end) = page_html[json_start..].find("</script>") {
                let json_str = &page_html[json_start..json_start + end];
                let data: Value = serde_json::from_str(json_str)?;

                if let Some(media_assets) = data.pointer("/props/pageProps/applicationData/mediaAssets") {
                    if let Some(url) = media_assets[0].get("url").and_then(|v| v.as_str()) {
                        return Ok(url.to_string());
                    }
                }
            }
        }

        Err(anyhow!("Failed to extract master playlist URL"))
    }

    async fn get_best_quality_stream(&self, master_url: &str) -> Result<String> {
        let master_playlist = self.client.get(master_url).send().await?.text().await?;
        let mut best_bandwidth = 0;
        let mut best_url: Option<String> = None; // Updated to Option<String>

        for line in master_playlist.lines() {
            if line.starts_with("#EXT-X-STREAM-INF") {
                if let Some(bandwidth) = line.split(',')
                    .find(|s| s.contains("BANDWIDTH="))
                    .and_then(|s| s.split('=').nth(1))
                    .and_then(|s| s.parse::<u32>().ok())
                {
                    if bandwidth > best_bandwidth {
                        best_bandwidth = bandwidth;
                        best_url = None; // Clear any previous best_url for the next URL line
                    }
                }
            } else if best_url.is_none() && !line.starts_with('#') {
                // Assign the line (URL) to best_url
                best_url = Some(line.to_string());
            }
        }

        let stream_path = best_url.ok_or_else(|| anyhow!("No streams found"))?;
        let base_url = Url::parse(master_url)?;
        let stream_url = base_url.join(&stream_path)?; // Pass directly as &String

        Ok(stream_url.to_string())
    }




    async fn get_media_segments(&self, playlist_url: &str) -> Result<Vec<(String, Option<(String, Vec<u8>)>)>> {
        let playlist = self.client.get(playlist_url).send().await?.text().await?;
        let base_url = Url::parse(playlist_url)?;
        let mut segments = Vec::new();
        let mut current_key_url: Option<String> = None;
        let mut current_iv: Option<Vec<u8>> = None;

        for line in playlist.lines() {
            if line.starts_with("#EXT-X-KEY:") {
                // Parse encryption info
                if line.contains("METHOD=AES-128") {
                    let uri = line.split("URI=\"").nth(1)
                        .and_then(|s| s.split("\"").next())
                        .ok_or_else(|| anyhow!("Invalid key URI"))?;
                    let iv = line.split("IV=0x").nth(1)
                        .and_then(|s| s.split(",").next())
                        .map(|hex| hex::decode(hex).unwrap_or_default());

                    current_key_url = Some(base_url.join(uri)?.to_string());
                    current_iv = iv;
                }
            } else if !line.starts_with("#") && !line.is_empty() {
                let segment_url = base_url.join(line)?;
                segments.push((
                    segment_url.to_string(),
                    current_key_url.as_ref().map(|key_url| (
                        key_url.clone(),
                        current_iv.clone().unwrap_or_else(|| vec![0; 16])
                    ))
                ));
            }
        }

        Ok(segments)
    }

    async fn get_api_playlist_url(&self, video_id: &str, token: &str, signature: &str) -> Result<String> {
        let api_url = "https://contentplayer.hotmart.com/video/content";
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();

        let response = self.client.post(api_url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://player.hotmart.com")
            .header("Referer", "https://player.hotmart.com/")
            .header("Accept", "application/json, text/plain, */*")
            .header("x-hotmart-app", "web-player")
            .header("x-hotmart-key", token)
            .json(&serde_json::json!({
                "videoId": video_id,
                "token": token,
                "timestamp": timestamp,
                "signature": signature,
                "captcha": serde_json::Value::Null,
                "locale": "en"
            }))
            .send()
            .await?;

        let body = response.text().await?;
        let config: Value = serde_json::from_str(&body)?;

        let master_url = config.pointer("/streaming/hls/url").or_else(|| config.pointer("/response/streaming/hls/url")).or_else(|| config.pointer("/data/streaming/hls/url")).and_then(|v| v.as_str()).ok_or_else(|| anyhow!("Could not find HLS URL in API response"))?;
        Ok(master_url.to_string())
    }
}
