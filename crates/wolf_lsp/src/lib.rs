//! The language server of wolfpack, `wolf lsp`: the diagnostics of `wolf check` while a file is
//! edited, the type or signature of a name on hover, going to the declaration of a name, and
//! formatting, over the Language Server Protocol on standard input and output.
//!
//! The server reads no files itself: `wolf_cli` gives it a function that checks the project around
//! a directory with the texts of the open documents instead of the files on disk. A file outside
//! any project is checked alone, as the playground checks its one file.

mod query;
mod text;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Formatting, GotoDefinition, HoverRequest, Request as _};
use lsp_types::{
    DiagnosticRelatedInformation, DiagnosticSeverity, DocumentFormattingParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, Location, MarkupContent, MarkupKind, NumberOrString, OneOf,
    PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, Url,
};
use wolf_diag::Diagnostic;
use wolf_span::{FileId, SourceMap, Span};

/// The texts of the open documents, by canonical path.
pub type Overlay = BTreeMap<PathBuf, String>;

/// The result of checking a project.
pub struct Checked {
    /// The files: the manifest, std and the modules.
    pub map: SourceMap,
    /// Every diagnostic.
    pub diagnostics: Vec<Diagnostic>,
    /// The directory of `wolfpack.toml`, when one was found; the names of the files of the
    /// project in the source map are relative to it.
    pub root: Option<PathBuf>,
    /// The checked project, when its manifest could be read.
    pub project: Option<Project>,
}

/// A checked project.
pub struct Project {
    /// The modules: their files and syntax trees.
    pub parsed: Vec<(FileId, wolf_ast::File)>,
    /// Name resolution.
    pub resolved: wolf_resolve::Resolved,
    /// Types.
    pub typed: wolf_types::Typed,
}

/// Checks the project around a directory, with the texts of the open documents.
pub type Check<'a> = dyn Fn(&Path, &Overlay) -> Checked + 'a;

/// Runs the server on standard input and output until the client asks it to exit.
pub fn serve(check: &Check<'_>) -> Result<(), String> {
    let (connection, threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    };
    let capabilities = serde_json::to_value(capabilities).map_err(|e| e.to_string())?;
    connection
        .initialize(capabilities)
        .map_err(|e| e.to_string())?;
    let mut server = Server {
        connection: &connection,
        check,
        docs: BTreeMap::new(),
        checked: BTreeMap::new(),
        published: BTreeMap::new(),
    };
    server.run()?;
    drop(connection);
    threads.join().map_err(|e| e.to_string())
}

/// The documents a client has open, and what was found in them.
struct Server<'a> {
    connection: &'a Connection,
    check: &'a Check<'a>,
    /// The texts of the open documents.
    docs: BTreeMap<Url, String>,
    /// The last check of each project (or lone file) by its directory, with the document of
    /// each file.
    checked: BTreeMap<PathBuf, (Checked, BTreeMap<FileId, Url>)>,
    /// The documents with diagnostics, by project.
    published: BTreeMap<PathBuf, BTreeSet<Url>>,
}

impl Server<'_> {
    fn run(&mut self) -> Result<(), String> {
        while let Ok(message) = self.connection.receiver.recv() {
            match message {
                Message::Request(request) => {
                    if self
                        .connection
                        .handle_shutdown(&request)
                        .map_err(|e| e.to_string())?
                    {
                        return Ok(());
                    }
                    let response = self.request(request);
                    self.send(Message::Response(response))?;
                }
                Message::Notification(n) => self.notification(n)?,
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn send(&self, message: Message) -> Result<(), String> {
        self.connection
            .sender
            .send(message)
            .map_err(|e| e.to_string())
    }

    fn notification(&mut self, n: Notification) -> Result<(), String> {
        let uri = match n.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let p: lsp_types::DidOpenTextDocumentParams = params(n.params)?;
                self.docs
                    .insert(p.text_document.uri.clone(), p.text_document.text);
                p.text_document.uri
            }
            DidChangeTextDocument::METHOD => {
                let p: lsp_types::DidChangeTextDocumentParams = params(n.params)?;
                if let Some(change) = p.content_changes.into_iter().last() {
                    self.docs.insert(p.text_document.uri.clone(), change.text);
                }
                p.text_document.uri
            }
            DidSaveTextDocument::METHOD => {
                let p: lsp_types::DidSaveTextDocumentParams = params(n.params)?;
                p.text_document.uri
            }
            DidCloseTextDocument::METHOD => {
                let p: lsp_types::DidCloseTextDocumentParams = params(n.params)?;
                self.docs.remove(&p.text_document.uri);
                p.text_document.uri
            }
            _ => return Ok(()),
        };
        self.recheck(&uri)
    }

    /// Checks the project of a document again and publishes the diagnostics of all its files.
    fn recheck(&mut self, uri: &Url) -> Result<(), String> {
        let Ok(path) = uri.to_file_path() else {
            return Ok(());
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let overlay: Overlay = self
            .docs
            .iter()
            .filter_map(|(u, text)| {
                let p = u.to_file_path().ok()?;
                Some((std::fs::canonicalize(&p).unwrap_or(p), text.clone()))
            })
            .collect();
        let mut checked = (self.check)(&dir, &overlay);
        let (key, files) = match &checked.root {
            Some(root) => {
                let files = project_files(&checked.map, root, &overlay);
                (root.clone(), files)
            }
            None => {
                // A file outside any project is checked alone; a closed one is forgotten.
                let Some(text) = self.docs.get(uri) else {
                    self.checked.remove(&path);
                    return self.publish(&path, BTreeMap::new());
                };
                let single = wolf_driver::single::check(text);
                let files: BTreeMap<FileId, Url> = single
                    .map
                    .iter()
                    .filter(|(_, f)| f.name() == wolf_driver::single::FILE)
                    .map(|(id, _)| (id, uri.clone()))
                    .collect();
                checked = Checked {
                    map: single.map,
                    diagnostics: single.diagnostics,
                    root: None,
                    project: None,
                };
                (path.clone(), files)
            }
        };
        let mut by_doc: BTreeMap<Url, Vec<lsp_types::Diagnostic>> =
            files.values().map(|u| (u.clone(), Vec::new())).collect();
        for d in &checked.diagnostics {
            let primary = d.labels.iter().find(|l| l.primary).or(d.labels.first());
            let (uri, range) =
                match primary.and_then(|l| files.get(&l.span.file).map(|u| (u, l.span))) {
                    Some((u, span)) => (u.clone(), span_range(&checked.map, span)),
                    // Diagnostics of the project itself go to its manifest.
                    None => match files
                        .values()
                        .find(|u| u.path().ends_with("/wolfpack.toml"))
                    {
                        Some(u) => (u.clone(), lsp_types::Range::default()),
                        None => continue,
                    },
                };
            by_doc
                .entry(uri)
                .or_default()
                .push(convert(&checked.map, &files, d, range));
        }
        self.checked.insert(key.clone(), (checked, files));
        self.publish(&key, by_doc)
    }

    /// Publishes the diagnostics of the documents of a project, and clears those of its
    /// documents that have none now.
    fn publish(
        &mut self,
        key: &Path,
        by_doc: BTreeMap<Url, Vec<lsp_types::Diagnostic>>,
    ) -> Result<(), String> {
        let before = self.published.remove(key).unwrap_or_default();
        let mut now = BTreeSet::new();
        for uri in before.iter().filter(|u| !by_doc.contains_key(*u)) {
            self.notify_diagnostics(uri.clone(), Vec::new())?;
        }
        for (uri, list) in by_doc {
            if !list.is_empty() {
                now.insert(uri.clone());
            }
            if !list.is_empty() || before.contains(&uri) || self.docs.contains_key(&uri) {
                self.notify_diagnostics(uri, list)?;
            }
        }
        self.published.insert(key.to_path_buf(), now);
        Ok(())
    }

    fn notify_diagnostics(
        &self,
        uri: Url,
        diagnostics: Vec<lsp_types::Diagnostic>,
    ) -> Result<(), String> {
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        };
        let params = serde_json::to_value(params).map_err(|e| e.to_string())?;
        self.send(Message::Notification(Notification::new(
            PublishDiagnostics::METHOD.to_string(),
            params,
        )))
    }

    fn request(&mut self, request: Request) -> Response {
        let id = request.id.clone();
        let result = match request.method.as_str() {
            HoverRequest::METHOD => params(request.params).and_then(|p| self.hover(&p)),
            GotoDefinition::METHOD => params(request.params).and_then(|p| self.definition(&p)),
            Formatting::METHOD => params(request.params).and_then(|p| self.format(&p)),
            _ => return error(id, ErrorCode::MethodNotFound, "unknown request"),
        };
        match result {
            Ok(value) => Response::new_ok(id, value),
            Err(message) => error(id, ErrorCode::InvalidParams, &message),
        }
    }

    /// The project and file of a document, and the byte offset of a position in it.
    fn locate(
        &self,
        uri: &Url,
        position: lsp_types::Position,
    ) -> Option<(&Checked, &BTreeMap<FileId, Url>, FileId, u32)> {
        let text = self.docs.get(uri)?;
        let offset = u32::try_from(text::offset(text, position)).ok()?;
        self.checked.values().find_map(|(checked, files)| {
            let file = files.iter().find(|(_, u)| *u == uri).map(|(f, _)| *f)?;
            Some((checked, files, file, offset))
        })
    }

    fn hover(&self, p: &HoverParams) -> Result<serde_json::Value, String> {
        let doc = &p.text_document_position_params;
        let found = self.locate(&doc.text_document.uri, doc.position).and_then(
            |(checked, _, file, offset)| {
                let project = checked.project.as_ref()?;
                let (named, span) = query::named_at(project, file, offset)?;
                let value = query::hover(project, named)?;
                Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: Some(span_range(&checked.map, span)),
                })
            },
        );
        serde_json::to_value(found).map_err(|e| e.to_string())
    }

    fn definition(&self, p: &GotoDefinitionParams) -> Result<serde_json::Value, String> {
        let doc = &p.text_document_position_params;
        let found = self.locate(&doc.text_document.uri, doc.position).and_then(
            |(checked, files, file, offset)| {
                let project = checked.project.as_ref()?;
                let (named, _) = query::named_at(project, file, offset)?;
                let span = query::declaration(project, named)?;
                let uri = files.get(&span.file)?.clone();
                Some(GotoDefinitionResponse::Scalar(Location::new(
                    uri,
                    span_range(&checked.map, span),
                )))
            },
        );
        serde_json::to_value(found).map_err(|e| e.to_string())
    }

    fn format(&self, p: &DocumentFormattingParams) -> Result<serde_json::Value, String> {
        let Some(text) = self.docs.get(&p.text_document.uri) else {
            return Ok(serde_json::Value::Null);
        };
        let mut map = SourceMap::new();
        let id = map.add("main.wolf", text.clone());
        // A file with syntax errors is left as it is; the diagnostics already show them.
        let edits = match wolf_fmt::format(id, text) {
            Ok(formatted) if formatted != *text => {
                vec![TextEdit::new(text::range(text, 0, text.len()), formatted)]
            }
            _ => Vec::new(),
        };
        serde_json::to_value(edits).map_err(|e| e.to_string())
    }
}

/// The documents of the files of a project that exist, on disk or open: its modules and its
/// manifest, not std.
fn project_files(map: &SourceMap, root: &Path, overlay: &Overlay) -> BTreeMap<FileId, Url> {
    map.iter()
        .filter_map(|(id, f)| {
            let path = root.join(f.name());
            let canonical = std::fs::canonicalize(&path).ok();
            let open = canonical.as_ref().is_some_and(|c| overlay.contains_key(c));
            if !open && !path.is_file() {
                return None;
            }
            Url::from_file_path(canonical.unwrap_or(path))
                .ok()
                .map(|u| (id, u))
        })
        .collect()
}

/// The range of a span in the protocol's positions.
fn span_range(map: &SourceMap, span: Span) -> lsp_types::Range {
    let text = map.get(span.file).map(|f| f.text()).unwrap_or("");
    text::range(text, span.start as usize, span.end as usize)
}

/// A diagnostic of the compiler as the protocol has it: the message with the notes and help, the
/// code, and the other labels as related places.
fn convert(
    map: &SourceMap,
    files: &BTreeMap<FileId, Url>,
    d: &Diagnostic,
    range: lsp_types::Range,
) -> lsp_types::Diagnostic {
    let mut message = d.message.clone();
    for note in &d.notes {
        message.push_str(&format!("\nnote: {note}"));
    }
    for help in &d.help {
        message.push_str(&format!("\nhelp: {help}"));
    }
    let related: Vec<DiagnosticRelatedInformation> = d
        .labels
        .iter()
        .filter(|l| !l.primary && !l.message.is_empty())
        .filter_map(|l| {
            let uri = files.get(&l.span.file)?.clone();
            Some(DiagnosticRelatedInformation {
                location: Location::new(uri, span_range(map, l.span)),
                message: l.message.clone(),
            })
        })
        .collect();
    lsp_types::Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(d.code.id().to_string())),
        source: Some("wolf".to_string()),
        message,
        related_information: (!related.is_empty()).then_some(related),
        ..lsp_types::Diagnostic::default()
    }
}

fn params<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn error(id: RequestId, code: ErrorCode, message: &str) -> Response {
    Response::new_err(id, code as i32, message.to_string())
}
