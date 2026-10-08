//! Every std module lexes and parses cleanly and states its own path.

use wolf_span::SourceMap;

#[test]
fn std_modules_parse() {
    for module in wolf_std::MODULES {
        let mut map = SourceMap::new();
        let id = map.add(module.file_name, module.source.to_string());
        let lexed = wolf_lexer::lex(id, module.source);
        let parsed = wolf_parser::parse(id, module.source, &lexed.tokens);
        let mut diagnostics = lexed.diagnostics;
        diagnostics.extend(parsed.diagnostics);
        let rendered = wolf_diag::render_human_to_string(&map, &diagnostics);
        assert!(diagnostics.is_empty(), "{rendered}");
        let path = parsed.file.module.map(|p| {
            p.segments
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
                .join(".")
        });
        assert_eq!(path.as_deref(), Some(module.path));
    }
}
