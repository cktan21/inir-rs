use crate::Context;
use regex::Regex;
use serde_json::{json, Value};
use std::{collections::HashSet, sync::LazyLock, time::Duration};

pub fn parse_lrc(lrc: &str) -> Vec<Value> {
    static STAMP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{2}(?:[.:]\d{1,3})?)\]").unwrap());
    let mut result = Vec::new();
    for raw in lrc.lines() {
        let stamps: Vec<_> = STAMP.captures_iter(raw).collect();
        let Some(last) = stamps.last() else {
            continue;
        };
        let text = raw[last.get(0).unwrap().end()..].trim();
        for stamp in stamps {
            if let (Ok(minutes), Ok(seconds)) = (
                stamp[1].parse::<f64>(),
                stamp[2].replace(':', ".").parse::<f64>(),
            ) {
                result.push(json!({"t": ((minutes * 60.0 + seconds) * 1000.0).round_ties_even() / 1000.0, "text": text}));
            }
        }
    }
    result.sort_by(|a, b| {
        a["t"]
            .as_f64()
            .unwrap()
            .total_cmp(&b["t"].as_f64().unwrap())
    });
    result
}
fn overlaps(wanted: &str, got: &str) -> bool {
    static PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\s]").unwrap());
    let wanted = PUNCTUATION
        .replace_all(&wanted.to_lowercase(), " ")
        .trim()
        .to_owned();
    let got = PUNCTUATION
        .replace_all(&got.to_lowercase(), " ")
        .trim()
        .to_owned();
    if wanted.is_empty() || got.is_empty() {
        return false;
    }
    if wanted.contains(&got) || got.contains(&wanted) {
        return true;
    }
    let words: HashSet<_> = got.split_whitespace().collect();
    wanted
        .split_whitespace()
        .filter(|w| w.chars().count() > 3)
        .any(|w| words.contains(w))
}
async fn get_json(context: &Context, url: reqwest::Url) -> Result<Value, String> {
    let client = context.http().await?;
    let mut response = client.get(url.clone()).send().await.map_err(|e| {
        if e.is_timeout() {
            "Lyrics request timed out"
        } else {
            "Lyrics request failed"
        }
    })?;
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let delay = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(1.0)
            .clamp(0.0, 60.0);
        tokio::time::sleep(Duration::from_secs_f64(delay)).await;
        response = client
            .get(url)
            .send()
            .await
            .map_err(|_| "Lyrics retry failed")?;
    }
    response
        .error_for_status()
        .map_err(|e| {
            format!(
                "Lyrics server returned HTTP {}",
                e.status().map(|s| s.as_u16()).unwrap_or(0)
            )
        })?
        .json()
        .await
        .map_err(|_| "Invalid lyrics response".into())
}
pub async fn fetch(context: &Context, args: &Value) -> Result<Value, String> {
    let title = args["title"].as_str().unwrap_or("").trim();
    let artist = args["artist"].as_str().unwrap_or("").trim();
    let album = args["album"].as_str().unwrap_or("").trim();
    let id = args["requestId"].as_str().unwrap_or("");
    let mut result = if title.is_empty() || artist.is_empty() {
        json!({"status": "no_info"})
    } else {
        let _permit = context.network_permit().await?;
        // An override is used only by isolated HTTP fixtures and explicit CLI callers.
        let base = args["api"]
            .as_str()
            .unwrap_or("https://lrclib.net/api")
            .trim_end_matches('/');
        let mut queries = Vec::new();
        if let Some(duration) = args["duration"]
            .as_f64()
            .filter(|d| *d != 0.0 && d.is_finite())
        {
            if !album.is_empty() {
                queries.push((
                    "get",
                    vec![
                        ("track_name", title.to_owned()),
                        ("artist_name", artist.to_owned()),
                        ("album_name", album.to_owned()),
                        ("duration", (duration as i64).to_string()),
                    ],
                ));
            }
        }
        queries.push((
            "search",
            vec![("track_name", title.into()), ("artist_name", artist.into())],
        ));
        queries.push(("search", vec![("q", format!("{title} {artist}"))]));
        let mut had_response = false;
        let mut last_error = String::new();
        let mut lines = Vec::new();
        for (index, (path, params)) in queries.into_iter().enumerate() {
            if index > 0 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let mut url = reqwest::Url::parse(&format!("{base}/{path}"))
                .map_err(|_| "Invalid lyrics endpoint")?;
            url.query_pairs_mut().extend_pairs(params);
            let payload = match get_json(context, url).await {
                Ok(value) => {
                    had_response = true;
                    value
                }
                Err(error) => {
                    last_error = error;
                    continue;
                }
            };
            let candidates = match payload {
                Value::Array(values) => values,
                value => vec![value],
            };
            for candidate in candidates {
                let Some(lrc) = candidate["syncedLyrics"].as_str().filter(|s| !s.is_empty()) else {
                    continue;
                };
                if !overlaps(title, candidate["trackName"].as_str().unwrap_or(""))
                    || !overlaps(artist, candidate["artistName"].as_str().unwrap_or(""))
                {
                    continue;
                }
                lines = parse_lrc(lrc);
                if !lines.is_empty() {
                    break;
                }
            }
            if !lines.is_empty() {
                break;
            }
        }
        if !lines.is_empty() {
            json!({"status": "ok", "lines": lines})
        } else if !had_response && !last_error.is_empty() {
            json!({"status": "error", "message": last_error})
        } else {
            json!({"status": "not_found"})
        }
    };
    if !id.is_empty() {
        result["requestId"] = json!(id);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_and_matching_preserve_existing_semantics() {
        assert_eq!(
            parse_lrc("[00:01.250][00:03:50] hello\n[00:00.00] 前奏"),
            vec![
                json!({"t":0.0,"text":"前奏"}),
                json!({"t":1.25,"text":"hello"}),
                json!({"t":3.5,"text":"hello"})
            ]
        );
        assert!(overlaps("Hello (Live)", "HELLO"));
        assert!(!overlaps("", "hello"));
    }

    async fn server(
        responses: Vec<(&'static str, &'static str, &'static str)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for (status, headers, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            requests
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn exact_query_retries_rate_limits_and_preserves_lyrics() {
        let (api, server) = server(vec![
            ("429 Too Many Requests", "Retry-After: 0\r\n", "{}"),
            ("200 OK", "", r#"{"trackName":"Hello (Live)","artistName":"Test Artist","syncedLyrics":"[00:01.25]first\n[00:03:50]second"}"#),
        ]).await;
        let result = fetch(&Context::default(), &json!({"api": api, "title":"Hello", "artist":"Test Artist", "album":"Album", "duration":120, "requestId":"one"})).await.unwrap();
        assert_eq!(
            result,
            json!({"status":"ok", "requestId":"one", "lines":[{"t":1.25,"text":"first"},{"t":3.5,"text":"second"}]})
        );
        let requests = server.await.unwrap();
        assert!(requests[0].starts_with(
            "GET /get?track_name=Hello&artist_name=Test+Artist&album_name=Album&duration=120 "
        ));
        assert_eq!(requests.len(), 2);
    }

    #[tokio::test]
    async fn searches_skip_unrelated_results_and_fall_back_to_full_query() {
        let (api, server) = server(vec![
            ("200 OK", "", r#"[{"trackName":"Wrong","artistName":"Test Artist","syncedLyrics":"[00:00.00]wrong"}]"#),
            ("200 OK", "", r#"[{"trackName":"Hello","artistName":"Test Artist","syncedLyrics":"[00:02.00]right"}]"#),
        ]).await;
        let result = fetch(
            &Context::default(),
            &json!({"api": api, "title":"Hello", "artist":"Test Artist"}),
        )
        .await
        .unwrap();
        assert_eq!(result["lines"][0]["text"], "right");
        let requests = server.await.unwrap();
        assert!(requests[1].starts_with("GET /search?q=Hello+Test+Artist "));
    }

    #[tokio::test]
    async fn cancellation_drops_inflight_http_instead_of_waiting_for_timeout() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let context = Context::default();
        let stop = tokio_util::sync::CancellationToken::new();
        let request = context.execute(
            "lyrics.fetch",
            json!({"api":api,"title":"Hello","artist":"Artist"}),
            &stop,
        );
        tokio::pin!(request);
        let (mut socket, _) = tokio::select! { result = listener.accept() => result.unwrap(), _ = &mut request => panic!("request finished before connecting") };
        stop.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_millis(200), request)
                .await
                .unwrap()
                .unwrap_err(),
            "Utility request canceled"
        );
        let mut bytes = [0; 4096];
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if socket.read(&mut bytes).await.unwrap_or(0) == 0 {
                    break;
                }
            }
        })
        .await
        .expect("Canceled HTTP connection was not closed");
    }
}
