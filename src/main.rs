use fantoccini::actions::{WheelAction, WheelActions};
use regex::Regex;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use webdriver::*;

// let's set up the sequence of steps we want the browser to take
#[tokio::main]
async fn main() -> Result<(), fantoccini::error::CmdError> {
    let client = create_client().await?;
    client.goto("https://www.pixiv.net/discovery").await?;
    add_cookies(&client).await?;
    sleep(Duration::new(2, 0));
    //创建保存文件的目录
    let title = create_dir_with_title(&client).await?;

    //准备好滚轮操作
    let mut wheel_actions = WheelActions::new("wheelactions".to_string());
    wheel_actions.push(WheelAction::Scroll {
        duration: (Some(Duration::from_millis(200))),
        x: (0),
        y: (0),
        delta_x: (0),
        delta_y: (5000),
    });

    //准备匹配出图片id
    let mut saved_ids = load_saved_urls();
    let regex = Regex::new(r"/(\d{4}/\d{2}/\d{2}/\d{2}/\d{2}/\d{2}/\d+)").unwrap();
    let mut matched_ids: Vec<String> = Vec::new();

    //获取单个页面的图片id
    for _ in 0..8{
        get_ids_in_one_page(&mut saved_ids, &client, &mut matched_ids, regex.clone()).await?;
        client.perform_actions(wheel_actions.clone()).await?;
        sleep(Duration::from_millis(1000));
    }
    println!("一共获取了{}个图片id", matched_ids.len());
    //准备reqwest客户端进行匹配url以及下载的过程；
    let re_client = create_re_client().await;

    // 并发匹配JPG/PNG,获取原图url
    let mut origin_urls = Vec::new();
    let jpg_counter = Arc::new(std::sync::Mutex::new(0));
    let png_counter = Arc::new(std::sync::Mutex::new(0));
    match_jpg_or_png(matched_ids, &mut origin_urls, re_client.clone(),jpg_counter,png_counter).await?;
    // 并发下载过程
    let mut futures = Vec::new();
    let mut counter: i32 = 0;
    for url in origin_urls {
        let path = format!("E:/pixiv/{}/image{}.png", title, counter);
        let future = download(re_client.clone(), path, url.clone());
        futures.push(future);
        counter += 1;
    }

    // 任务执行过程
    update_progress_bar(futures).await.expect("download_error");
    client.persist().await
}
