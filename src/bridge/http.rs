use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::{rpc, ws, BridgeServer};

const MAX_HTTP_BODY_BYTES: usize = 2 * 1024 * 1024;

include!(concat!(env!("OUT_DIR"), "/embedded_web_assets.rs"));

pub(crate) fn find_screenz_web_dist_dir() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("ScreenZWeb/dist"),
        PathBuf::from("../ScreenZWeb/dist"),
        PathBuf::from("../../ScreenZWeb/dist"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ScreenZWeb/dist"));
            candidates.push(dir.join("../../ScreenZWeb/dist"));
            candidates.push(dir.join("../../../ScreenZWeb/dist"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.join("index.html").is_file())
}

fn content_type_for_path(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "json" => "application/json; charset=utf-8",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

fn find_embedded_asset(rel: &str) -> Option<&'static [u8]> {
    EMBEDDED_WEB_ASSETS
        .iter()
        .find(|(p, _)| *p == rel)
        .map(|(_, bytes)| *bytes)
}

fn write_static_response(stream: &mut TcpStream, rel: &str, bytes: &[u8]) {
    let ct = content_type_for_path(Path::new(rel));
    let cache = if rel.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nCache-Control: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ct,
        cache,
        bytes.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(bytes);
}

fn write_json_response(stream: &mut TcpStream, status: &str, json_body: &str) {
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        json_body.len(),
        json_body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn resolve_static_path(root: &Path, relative_path: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let target = root.join(relative_path).canonicalize().ok()?;
    (target.starts_with(&root) && target.is_file()).then_some(target)
}

fn try_serve_static(stream: &mut TcpStream, req_path: &str, dist_dir: Option<&Path>) {
    let clean_path = req_path.split('?').next().unwrap_or("/");

    if !EMBEDDED_WEB_ASSETS.is_empty() {
        let rel = clean_path.trim_start_matches('/');
        let (rel, bytes) = match find_embedded_asset(rel) {
            Some(b) if !rel.is_empty() => (rel, b),
            _ => (
                "index.html",
                find_embedded_asset("index.html").unwrap_or_default(),
            ),
        };
        write_static_response(stream, rel, bytes);
        return;
    }

    let Some(root) = dist_dir.and_then(|path| path.canonicalize().ok()) else {
        let html = "<html><body style='background:#111214;color:#f2f3f5;font-family:sans-serif;padding:32px'><h2>ScreenZ Backend Ativo (:47839)</h2><p>Rode <code>cd ScreenZWeb && npm run build</code> para servir o frontend estático aqui, ou acesse <code>http://localhost:5173</code> em modo dev.</p></body></html>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    };

    let rel = clean_path.trim_start_matches('/');
    let Some(target) =
        resolve_static_path(&root, rel).or_else(|| resolve_static_path(&root, "index.html"))
    else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };

    match fs::read(&target) {
        Ok(bytes) => {
            let rel = target
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            write_static_response(stream, &rel, &bytes);
        }
        Err(_) => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    }
}

fn parse_content_length(headers: &str) -> Result<usize, &'static str> {
    let mut content_length = None;
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            return Err("Cabeçalho inválido");
        };
        if name.is_empty() || name.trim() != name {
            return Err("Nome de cabeçalho inválido");
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("Transfer-Encoding não suportado");
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if content_length.is_some()
                || value.is_empty()
                || !value.bytes().all(|b| b.is_ascii_digit())
            {
                return Err("Content-Length inválido");
            }
            content_length = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| "Content-Length inválido")?,
            );
        }
    }
    Ok(content_length.unwrap_or(0))
}

fn read_http_body(
    reader: &mut impl Read,
    buffered: &[u8],
    length: usize,
) -> std::io::Result<String> {
    let mut body = vec![0; length];
    let buffered_length = buffered.len().min(length);
    body[..buffered_length].copy_from_slice(&buffered[..buffered_length]);
    reader.read_exact(&mut body[buffered_length..])?;
    String::from_utf8(body)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn parse_ws_tile_filter(path: &str) -> Result<Option<u32>, &'static str> {
    let Some((_, query)) = path.split_once('?') else {
        return Ok(None);
    };
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "tileId" {
                let id = v.parse::<u32>().map_err(|_| "tileId inválido")?;
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

pub(crate) fn handle_http_connection(mut stream: TcpStream, bridge: Arc<BridgeServer>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 2048];

    let header_end = loop {
        match stream.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    if pos > 65_536 {
                        return;
                    }
                    break pos;
                }
                if buf.len() > 65_536 {
                    return;
                }
            }
            Err(_) => return,
        }
    };

    let header_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = header_str.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let content_len = match parse_content_length(&header_str) {
        Ok(length) => length,
        Err(error) => {
            let body = serde_json::json!({ "ok": false, "error": error }).to_string();
            write_json_response(&mut stream, "400 Bad Request", &body);
            return;
        }
    };
    let mut ws_key: Option<String> = None;
    let mut is_upgrade = false;

    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let kl = k.trim().to_ascii_lowercase();
            let vt = v.trim();
            if kl == "sec-websocket-key" {
                ws_key = Some(vt.to_string());
            } else if kl == "upgrade" && vt.eq_ignore_ascii_case("websocket") {
                is_upgrade = true;
            }
        }
    }

    if method == "GET" && is_upgrade && path.split('?').next() == Some("/ws") {
        let Some(key) = ws_key else {
            return;
        };
        match parse_ws_tile_filter(path) {
            Ok(filter_tile_id) => ws::serve_websocket_client(stream, &key, filter_tile_id),
            Err(_) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        }
        return;
    }

    if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    if method == "GET" && !path.starts_with("/api/") {
        try_serve_static(&mut stream, path, bridge.dist_dir.as_deref());
        return;
    }

    if content_len > MAX_HTTP_BODY_BYTES {
        write_json_response(
            &mut stream,
            "413 Payload Too Large",
            r#"{"ok":false,"error":"Payload muito grande"}"#,
        );
        return;
    }

    let body_start = header_end + 4;
    let body_str = match read_http_body(&mut stream, &buf[body_start..], content_len) {
        Ok(body) => body,
        Err(_) => {
            write_json_response(
                &mut stream,
                "400 Bad Request",
                r#"{"ok":false,"error":"Corpo HTTP incompleto ou inválido"}"#,
            );
            return;
        }
    };

    let (status, json_body) = if method == "GET" && path == "/api/state" {
        ("200 OK", bridge.get_snapshot_json())
    } else if method == "POST" && path == "/api/rpc" {
        match rpc::handle_rpc_request(&bridge, &body_str) {
            Ok(res_json) => ("200 OK", format!(r#"{{"ok":true,"data":{}}}"#, res_json)),
            Err(err_msg) => {
                let err_json =
                    serde_json::to_string(&err_msg).unwrap_or_else(|_| "\"Erro\"".to_string());
                (
                    "400 Bad Request",
                    format!(r#"{{"ok":false,"error":{}}}"#, err_json),
                )
            }
        }
    } else {
        (
            "404 Not Found",
            r#"{"ok":false,"error":"Not found"}"#.to_string(),
        )
    };

    write_json_response(&mut stream, status, &json_body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_screenz_web_embutida_no_executavel() {
        let index = find_embedded_asset("index.html").expect("index.html deve estar embutido");
        let html = std::str::from_utf8(index).unwrap();
        for part in html.split('"').filter(|s| s.starts_with("/assets/")) {
            assert!(
                find_embedded_asset(part.trim_start_matches('/')).is_some(),
                "asset ausente: {part}"
            );
        }
        assert_eq!(
            content_type_for_path(Path::new("assets/x.js")),
            "application/javascript; charset=utf-8"
        );
    }

    #[test]
    fn valida_tile_id_strict() {
        assert_eq!(parse_ws_tile_filter("/ws"), Ok(None));
        assert_eq!(parse_ws_tile_filter("/ws?tileId=0"), Ok(Some(0)));
        assert!(parse_ws_tile_filter("/ws?tileId=abc").is_err());
    }

    #[test]
    fn valida_content_length_sem_ambiguidade() {
        assert_eq!(
            parse_content_length("GET / HTTP/1.1\r\nHost: localhost"),
            Ok(0)
        );
        assert_eq!(
            parse_content_length("POST / HTTP/1.1\r\ncontent-length: 12"),
            Ok(12)
        );
        for headers in [
            "Content-Length: -1",
            "Content-Length: +12",
            "Content-Length: abc",
            "Content-Length:",
            "Content-Length: 9999999999999999999999999999",
            "Content-Length: 2\r\nContent-Length: 3",
            "Transfer-Encoding: chunked",
            "Content-Length : 2",
        ] {
            assert!(parse_content_length(&format!("POST / HTTP/1.1\r\n{headers}")).is_err());
        }
    }

    #[test]
    fn corpo_http_respeita_tamanho_e_rejeita_truncamento() {
        use std::io::Cursor;

        assert_eq!(
            read_http_body(&mut Cursor::new(b"cdextra"), b"ab", 4).unwrap(),
            "abcd"
        );
        assert_eq!(
            read_http_body(&mut Cursor::new([]), b"abcdextra", 4).unwrap(),
            "abcd"
        );
        assert!(read_http_body(&mut Cursor::new(b"c"), b"ab", 4).is_err());
        assert!(read_http_body(&mut Cursor::new([]), &[0xff], 1).is_err());
    }

    #[test]
    fn arquivos_estaticos_ficam_dentro_do_dist() {
        let workspace = std::env::temp_dir().join(format!("screenz-static-{}", std::process::id()));
        let root = workspace.join("dist");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("index.html"), "index").unwrap();
        fs::write(workspace.join("outside.txt"), "outside").unwrap();

        assert!(resolve_static_path(&root, "index.html").is_some());
        assert!(resolve_static_path(&root, "../outside.txt").is_none());
        assert!(
            resolve_static_path(&root, &workspace.join("outside.txt").to_string_lossy()).is_none()
        );
        assert!(resolve_static_path(&root, "missing.js").is_none());

        fs::remove_file(workspace.join("outside.txt")).unwrap();
        fs::remove_file(root.join("index.html")).unwrap();
        fs::remove_dir(root).unwrap();
        fs::remove_dir(workspace).unwrap();
    }

    #[test]
    fn resposta_json_usa_tamanho_em_bytes() {
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let body = r#"{"ok":false,"error":"Requisição inválida"}"#;
        write_json_response(&mut server, "400 Bad Request", body);
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.contains(&format!("Content-Length: {}\r\n", body.len())));
        assert!(response.ends_with(body));
    }
}
