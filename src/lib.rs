use fantoccini::error::CmdError;
use fantoccini::{ClientBuilder, Locator};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;
#[derive(Deserialize)]
#[allow(dead_code)]
#[allow(non_snake_case)]
pub struct JsonCookie {
    domain: String,
    expirationDate: Option<f64>,
    hostOnly: bool,
    httpOnly: bool,
    name: String,
    path: String,
    sameSite: String,
    secure: bool,
    session: bool,
    storeId: String,
    value: String,
    id: i32,
}

pub async fn create_client() -> Result<fantoccini::Client, CmdError> {
    let mut caps = fantoccini::wd::Capabilities::new();
    caps.insert(
        "goog:chromeOptions".to_string(),
        serde_json::json!({
            "args": [
                "--headless", // 启用无头模式
                "--disable-gpu",
                "--disable-dev-shm-usage",
            ],
        }),
    );
    Ok(ClientBuilder::rustls()
        .expect("rustls")
        .capabilities(caps)
        .connect("http://localhost:55928")
        .await
        .expect("failed to connect to WebDriver"))
}
pub async fn add_cookies(client: &fantoccini::Client) -> Result<(), fantoccini::error::CmdError> {
    use std::io::{Read, Write};

    println!("输入cookies：（Ctrl+Z结束）");
    std::io::stdout().flush().unwrap();
    let mut json_cookies = String::new();
    std::io::stdin().read_to_string(&mut json_cookies).unwrap();
    let cookies: Vec<JsonCookie> = serde_json::from_str(&json_cookies).unwrap();
    let cookies: Vec<cookie::Cookie<'static>> = cookies
        .into_iter()
        .map(|cookie| {
            cookie::Cookie::build((cookie.name, cookie.value))
                .domain(cookie.domain)
                .path(cookie.path)
                .http_only(cookie.httpOnly)
                .secure(cookie.secure)
                .build()
        })
        .collect();
    for c in cookies {
        client.add_cookie(c).await?;
    }
    client.refresh().await?;
    Ok(())
}
pub async fn create_dir_with_title(client: &fantoccini::Client) -> Result<String, CmdError> {
    let title = client.title().await.expect("获取标题失败");
    let title = title
        .replace("/", "-")
        .replace(":", "-")
        .replace("*", "-")
        .replace("?", "-")
        .replace("\"", "-")
        .replace("<", "-")
        .replace(">", "-")
        .replace("|", "-");
    std::fs::create_dir_all(format!("E:/pixiv/{}", &title)).expect("创建文件夹失败");
    Ok(title)
}

const SAVED_URLS_FILE: &str = "saved_ids.txt";

// 从文件中读取已保存的URLs到HashSet中
pub fn load_saved_urls() -> HashSet<String> {
    let path = Path::new(SAVED_URLS_FILE);
    if path.exists() {
        let data = fs::read_to_string(path).expect("Unable to read file");
        data.lines().map(|s| s.to_string()).collect()
    } else {
        HashSet::new()
    }
}
// 将HashSet中的URLs写入文件
pub fn save_hashset(urls: &HashSet<String>) {
    let path = Path::new(SAVED_URLS_FILE);
    let data: Vec<String> = urls.iter().cloned().collect();
    fs::write(path, data.join("\n")).expect("Unable to write file");
}

pub async fn get_ids_in_one_page(
    saved_ids: &mut HashSet<String>,
    client: &fantoccini::Client,
    matched_ids: &mut Vec<String>,
    regex: regex::Regex,
) -> Result<(), CmdError> {
    let images = client.find_all(Locator::Css("img")).await?;
    for image in images {
        let src = image.attr("src").await?;
        if let Some(url) = src {
            if url.contains("user-profile") {
                continue;
            }
            if let Some(captures) = regex.captures(&url) {
                let matched_string = captures.get(1).map_or("", |m| m.as_str()).to_string();
                if !saved_ids.contains(&matched_string) {
                    saved_ids.insert(matched_string.clone());
                    matched_ids.push(matched_string);
                }
            } else {
                continue;
            }
        }
    }
    save_hashset(&saved_ids);
    println!("获取了{}个图片id", matched_ids.len());
    Ok(())
}

pub async fn create_re_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0".parse().unwrap());
    headers.insert(
        reqwest::header::REFERER,
        "https://www.pixiv.com".parse().expect("referer"),
    );

    reqwest::Client::builder()
        .pool_max_idle_per_host(50)
        .default_headers(headers)
        .build()
        .expect("re_client error")
}

pub async fn match_jpg_or_png(
    matched_ids: Vec<String>,
    origin_urls: &mut Vec<String>,
    re_client: reqwest::Client,
    jpg_counter: Arc<Mutex<i32>>,
    png_counter: Arc<Mutex<i32>>,
) -> Result<(), CmdError> {
    let mut tasks = JoinSet::new();
    let unmatched_ids = Arc::new(Mutex::new(Vec::new()));
    for id in matched_ids.clone() {
        let re_client = re_client.clone();
        let jpg_counter = Arc::clone(&jpg_counter);
        let png_counter = Arc::clone(&png_counter);
        let unmatched_ids = Arc::clone(&unmatched_ids);
        tasks.spawn(async move {
            let base_url = format!("https://i.pximg.net/img-original/img/{}_p0", id);
            let jpg_url = format!("{}.jpg", base_url);
            let png_url = format!("{}.png", base_url);
            let png_head_response = re_client.head(&png_url).send().await;

            if let Ok(png_response) = png_head_response {
                if let Some(content_type) =
                    png_response.headers().get(reqwest::header::CONTENT_TYPE)
                {
                    if content_type.to_str().expect("png").starts_with("image/png") {
                        let mut png_counter = png_counter.lock().unwrap();
                        *png_counter += 1;
                        return Some(png_url);
                    }
                }
            }

            let jpg_head_response = re_client.head(&jpg_url).send().await;
            if let Ok(jpg_response) = jpg_head_response {
                if let Some(content_type) =
                    jpg_response.headers().get(reqwest::header::CONTENT_TYPE)
                {
                    if content_type
                        .to_str()
                        .expect("jpg")
                        .starts_with("image/jpeg")
                    {
                        let mut jpg_counter = jpg_counter.lock().unwrap();
                        *jpg_counter += 1;
                        return Some(jpg_url);
                    }
                }
            }
            let mut unmatched = unmatched_ids.lock().unwrap();
            unmatched.push(id);
            None
        });
    }

    // 等待所有匹配任务完成
    while let Some(result) = tasks.join_next().await {
        if let Ok(Some(url)) = result {
            origin_urls.push(url);
        }
    }
    let jpg_counter = *jpg_counter.lock().unwrap();
    let png_counter = *png_counter.lock().unwrap();
    println!(
        "一共获取了{}个jpg原图url,{}个png原图",
        jpg_counter, png_counter
    );
    println!("一共获取了{}个原图url", origin_urls.len());

    // 打印未匹配的图片 ID
    let unmatched = unmatched_ids.lock().unwrap();
    if !unmatched.is_empty() {
        println!("未匹配到的图片ID：");
        for id in unmatched.iter() {
            println!("{}", id);
        }
    }
    Ok(())
}

pub async fn download(
    client: reqwest::Client,
    file: String,
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let re = client.get(url).send().await?;
    let bytes = re.bytes().await?;
    let mut output = tokio::fs::File::create(file).await?;
    output.write_all(&bytes).await?;
    Ok(())
}
pub async fn update_progress_bar(
    futures: Vec<impl std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let pb = ProgressBar::new(futures.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({eta})")
            .progress_chars("#>-"),
    );
    let stream = stream::iter(futures);
    stream
        .for_each_concurrent(None, |f| async {
            match f.await {
                Ok(_) => {}
                Err(e) => eprintln!("下载失败：{}", e),
            }
            pb.inc(1);
        })
        .await;
    pb.finish_with_message("下载完成");
    Ok(())
}

pub async fn log_in(client: &fantoccini::Client) -> Result<(), fantoccini::error::CmdError> {
    client
        .wait()
        .for_element(Locator::Css(
            "#wrapper > div.signup-form > div > div:nth-child(2) > a.signup-form__submit--login",
        ))
        .await?
        .click()
        .await?;
    client.wait().for_element(Locator::Css("#app-mount-point > div > div > div.sc-fvq2qx-4.bVIVOB > div.sc-2oz7me-0.bOKfsa > form > fieldset.sc-bn9ph6-0.bYwpCj.sc-2o1uwj-3.bzFkbp > label > input")).await?.send_keys("2922693363@qq.com").await?;
    client.wait().for_element(Locator::Css("#app-mount-point > div > div > div.sc-fvq2qx-4.bVIVOB > div.sc-2oz7me-0.bOKfsa > form > fieldset.sc-bn9ph6-0.bYwpCj.sc-2o1uwj-4.fiHJkI > label > input")).await?.send_keys("JC3224948275").await?;
    client.find(Locator::Css("#app-mount-point > div > div > div.sc-fvq2qx-4.bVIVOB > div.sc-2oz7me-0.bOKfsa > form > button.sc-aXZVg.fSnEpf.sc-eqUAAy.hhGKQA.sc-2o1uwj-8.dTBiMW.sc-2o1uwj-8.dTBiMW")).await?.click().await?;
    Ok(())
}
