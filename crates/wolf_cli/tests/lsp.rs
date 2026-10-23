//! `wolf lsp` speaks the Language Server Protocol on standard input and output: the diagnostics
//! of the open documents, hover, go to definition and formatting.

// Test helpers may fail loudly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: i64,
    /// Notifications received while waiting for responses.
    notifications: Vec<Value>,
}

impl Client {
    fn start(dir: &Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wolf"))
            .arg("lsp")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            stdin,
            stdout,
            next: 1,
            notifications: Vec::new(),
        }
    }

    fn write(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut length = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                length = n.parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.read();
            if message.get("id") == Some(&json!(id)) {
                return message["result"].clone();
            }
            self.notifications.push(message);
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// The next diagnostics published for `uri`.
    fn diagnostics(&mut self, uri: &str) -> Vec<Value> {
        loop {
            let message = if self.notifications.is_empty() {
                self.read()
            } else {
                self.notifications.remove(0)
            };
            if message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == uri
            {
                return message["params"]["diagnostics"].as_array().unwrap().clone();
            }
        }
    }
}

/// The position of the `n`th occurrence (from 0) of `needle` in `text`, plus `skip` characters.
fn at(text: &str, needle: &str, n: usize, skip: u32) -> Value {
    let offset = text.match_indices(needle).nth(n).unwrap().0;
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1);
    json!({"line": line, "character": column as u32 + skip})
}

const FIXED: &str = r#"module main

types {
    /// A point on a plane.
    struct Point {
        x: Int
        y: Int
    }
}

main {
    let p = Point { x: 1, y: 2 }
    print(sum(p).to_string())
}

functions {
    fn sum(p: Point) -> Int {
        p.x + p.y
    }
}
"#;

#[test]
fn the_language_server_checks_explains_finds_and_formats() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("lsp")
        .join("demo");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("wolfpack.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nkind = \"cli\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.wolf"), FIXED).unwrap();
    let main = fs::canonicalize(dir.join("src/main.wolf")).unwrap();
    let uri = url::Url::from_file_path(&main).unwrap().to_string();

    let mut client = Client::start(&dir);
    let init = client.request(
        "initialize",
        json!({"processId": null, "rootUri": null, "capabilities": {}}),
    );
    assert_eq!(init["capabilities"]["hoverProvider"], json!(true));
    client.notify("initialized", json!({}));

    // A document with an error, as the editor has it: the file on disk is not read.
    let broken = FIXED.replace("print(sum(p)", "print(sun(p)");
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "wolfpack", "version": 1, "text": broken}}),
    );
    let found = client.diagnostics(&uri);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0]["code"], "E0609");
    assert_eq!(found[0]["range"]["start"], at(&broken, "sun", 0, 0));

    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": 2}, "contentChanges": [{"text": FIXED}]}),
    );
    assert_eq!(client.diagnostics(&uri), Vec::<Value>::new());

    let hover = |client: &mut Client, position: Value| -> String {
        let result = client.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": position}),
        );
        result["contents"]["value"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let call = hover(&mut client, at(FIXED, "sum(p)", 0, 1));
    assert!(call.contains("fn sum(p: Point) -> Int"), "{call}");
    let local = hover(&mut client, at(FIXED, "sum(p)", 0, 4));
    assert!(local.contains("p: Point"), "{local}");
    let ty = hover(&mut client, at(FIXED, "Point {", 1, 2));
    assert!(ty.contains("struct Point { x: Int, y: Int }"), "{ty}");
    assert!(ty.contains("A point on a plane."), "{ty}");

    let definition = client.request(
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": at(FIXED, "sum(p)", 0, 1)}),
    );
    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        at(FIXED, "sum(p: Point)", 0, 0)
    );

    let messy = FIXED.replace(
        "    let p = Point { x: 1, y: 2 }",
        "  let p = Point{x:1,y:2}",
    );
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": uri, "version": 3}, "contentChanges": [{"text": messy}]}),
    );
    let edits = client.request(
        "textDocument/formatting",
        json!({"textDocument": {"uri": uri}, "options": {"tabSize": 4, "insertSpaces": true}}),
    );
    assert_eq!(edits[0]["newText"], FIXED, "{edits}");

    assert_eq!(client.request("shutdown", Value::Null), Value::Null);
    client.notify("exit", Value::Null);
    let status = client.child.wait().unwrap();
    assert!(status.success(), "{status:?}");
}
