use std::path::Path;

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::folding;

/// Folding ranges as `start-end kind`.
fn folds(src: &str) -> Vec<String> {
    let path = Path::new("test/Kconfig");
    let mut index = WorldIndex::new();
    index.analyze_file(path, src);
    folding::folding_ranges(&index, path)
        .unwrap()
        .iter()
        .map(|r| format!("{}-{} {:?}", r.start_line, r.end_line, r.kind))
        .collect()
}

#[test]
fn blocks_fold_up_to_their_end_line_and_help_text_folds() {
    let src = "\
menu \"Drivers\"

config A
\tbool \"a\"
\thelp
\t  Line one.
\t  Line two.

if A

choice
\tprompt \"Pick\"
\thelp
\t  Pick one.

config C1
\tbool \"C1\"

endchoice

endif

endmenu

menu \"Empty\"
endmenu

comment \"Note\"

config B
\tbool \"b\"
\thelp
\t  One line.
";
    assert_eq!(
        folds(src),
        [
            "0-21 None",
            "4-6 Some(Comment)",
            "8-19 None",
            "10-17 None",
            "12-13 Some(Comment)",
            "31-32 Some(Comment)",
        ]
    );
}

#[test]
fn block_without_end_keyword_folds_to_its_last_line() {
    for src in [
        "menu \"M\"\nconfig A\n\tbool \"a\"",
        "menu \"M\"\nconfig A\n\tbool \"a\"\n\n",
        "if B\nconfig A\n\tdefault FOO_endif",
    ] {
        assert_eq!(folds(src), ["0-2 None"], "{src:?}");
    }
}

#[test]
fn no_folding_ranges_for_unknown_file() {
    let index = WorldIndex::new();
    assert!(folding::folding_ranges(&index, Path::new("test/Kconfig")).is_none());
}
