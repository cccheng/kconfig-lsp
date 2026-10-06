use std::path::{Path, PathBuf};

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::settings::Settings;
use kconfig_lsp::symbols;
use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Range};

fn index(src: &str) -> (WorldIndex, PathBuf) {
    let path = Path::new("test/Kconfig").to_path_buf();
    let mut index = WorldIndex::new();
    index.settings = Settings {
        zephyr_extensions: true,
    };
    index.analyze_file(&path, src);
    (index, path)
}

fn pos(r: Range) -> String {
    format!(
        "{}:{}-{}:{}",
        r.start.line, r.start.character, r.end.line, r.end.character
    )
}

/// One line per symbol, indented by depth: name, kind, detail, range and
/// selection range.
fn outline(src: &str) -> Vec<String> {
    fn walk(symbols: &[DocumentSymbol], depth: usize, out: &mut Vec<String>) {
        for s in symbols {
            out.push(format!(
                "{}{} {:?} {:?} {} {}",
                "  ".repeat(depth),
                s.name,
                s.kind,
                s.detail,
                pos(s.range),
                pos(s.selection_range)
            ));
            walk(s.children.as_deref().unwrap_or_default(), depth + 1, out);
        }
    }
    let (index, path) = index(src);
    let Some(DocumentSymbolResponse::Nested(symbols)) = symbols::document_symbols(&index, &path)
    else {
        panic!("no nested symbols");
    };
    let mut out = Vec::new();
    walk(&symbols, 0, &mut out);
    out
}

#[test]
fn outline_nests_entries_in_menus_choices_and_ifs() {
    let src = "\
mainmenu \"Main\"

config A
\tbool \"Use A 😀\"

menu \"Drivers\"
comment \"Some drivers\"
source \"drivers/Kconfig\"

if  !A &&\tD
menuconfig B
\tbool
\tprompt \"B prompt\"
endif

choice
\tprompt \"Pick one\"
config C1
\tbool \"C1\"
endchoice

endmenu

configdefault A
\tdefault y
";
    assert_eq!(
        outline(src),
        [
            // `😀` is 2 UTF-16 units.
            "A Variable Some(\"Use A 😀\") 2:0-3:16 2:7-2:8",
            "Drivers Module None 5:0-21:7 5:5-5:14",
            "  if !A && D Namespace None 9:0-13:5 9:5-9:11",
            "    B Variable Some(\"B prompt\") 10:0-12:18 10:11-10:12",
            "  Pick one Enum None 15:0-19:9 15:0-15:6",
            "    C1 Variable Some(\"C1\") 17:0-18:10 17:7-17:9",
            "A Variable Some(\"configdefault\") 23:0-24:10 23:14-23:15",
        ]
    );
}

#[test]
fn outline_names_unnamed_blocks_by_keyword() {
    // A prompt of only blanks is no name either.
    let src = "choice\nconfig C1\n\tbool \"C1\"\nendchoice\n\
               choice\n\tprompt \" \"\nendchoice\n\
               menu \" \"\nendmenu\n\
               config\n";
    assert_eq!(
        outline(src),
        [
            "choice Enum None 0:0-3:9 0:0-0:6",
            "  C1 Variable Some(\"C1\") 1:0-2:10 1:7-1:9",
            "choice Enum None 4:0-6:9 4:0-4:6",
            "menu Module None 7:0-8:7 7:5-7:8",
        ]
    );
}
