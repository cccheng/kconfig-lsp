use std::path::{Path, PathBuf};

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::settings::Settings;
use kconfig_lsp::{definition, links};
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Range, Url};

/// An empty directory for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kconfig-lsp-links-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// An index of `rel` in a workspace at `root`, and the path of `rel`.
fn index_of(root: &Path, rel: &str) -> (WorldIndex, PathBuf) {
    let mut index = WorldIndex::new();
    index.settings = Settings {
        zephyr_extensions: true,
        ..Default::default()
    };
    index.root = Some(root.to_path_buf());
    let path = root.join(rel);
    let source = std::fs::read_to_string(&path).unwrap();
    index.analyze_file(&path, &source);
    (index, path)
}

/// The links of `rel` in a workspace at `root`, as the linked text and
/// the target relative to `root`.
fn links_of(root: &Path, rel: &str) -> Vec<(String, String)> {
    let (index, path) = index_of(root, rel);
    let fa = &index.files[&path];
    let source = &fa.source;
    links::document_links(&index, &path)
        .unwrap()
        .into_iter()
        .map(|link| {
            let r = link.range;
            let start = fa
                .line_index
                .offset(source, r.start.line, r.start.character);
            let end = fa.line_index.offset(source, r.end.line, r.end.character);
            let target = link.target.unwrap().to_file_path().unwrap();
            let target = target.strip_prefix(root).unwrap().to_string_lossy();
            (source[start..end].to_string(), target.replace('\\', "/"))
        })
        .collect()
}

#[test]
fn source_paths_link_to_their_files() {
    let root = temp_dir("paths");
    write(&root, "a/Kconfig", "");
    write(&root, "sub/b/Kconfig", "");
    write(&root, "Kconfig_a", "");
    write(
        &root,
        "sub/Kconfig",
        "SUB := a\n\
         source \"a/Kconfig\"\n\
         rsource 'b/Kconfig'\n\
         source Kconfig_a\n\
         menu \"é\"\n\
         \tsource \"$(SUB)/Kconfig\" # é\n\
         endmenu\n",
    );
    let link = |text: &str, target: &str| (text.to_string(), target.to_string());
    assert_eq!(
        links_of(&root, "sub/Kconfig"),
        [
            link("a/Kconfig", "a/Kconfig"),
            link("b/Kconfig", "sub/b/Kconfig"),
            link("Kconfig_a", "Kconfig_a"),
            link("$(SUB)/Kconfig", "a/Kconfig"),
        ]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn link_range_counts_utf16_units() {
    let root = temp_dir("utf16");
    write(&root, "é😀/Kconfig", "");
    write(&root, "Kconfig", "source \"é😀/Kconfig\"\n");
    let (index, path) = index_of(&root, "Kconfig");
    let links = links::document_links(&index, &path).unwrap();
    let ranges: Vec<Range> = links.iter().map(|l| l.range).collect();
    // `é` is 1 UTF-16 unit and `😀` is 2.
    assert_eq!(
        ranges,
        [Range::new(Position::new(0, 8), Position::new(0, 19))]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn path_that_names_no_file_or_many_files_gets_no_link() {
    let root = temp_dir("none");
    write(&root, "a/Kconfig", "");
    write(&root, "b/Kconfig", "");
    write(
        &root,
        "Kconfig",
        "source \"missing/Kconfig\"\n\
         source \"*/Kconfig\"\n\
         source \"$(KCONFIG_LSP_NO_SUCH_VAR)/Kconfig\"\n\
         source \"a\"\n",
    );
    assert_eq!(links_of(&root, "Kconfig"), []);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn no_links_for_a_file_not_in_the_index() {
    let index = WorldIndex::new();
    assert!(links::document_links(&index, Path::new("/no/such/Kconfig")).is_none());
}

#[test]
fn goto_on_a_source_path_opens_its_files() {
    let root = temp_dir("goto");
    write(&root, "a/Kconfig", "config A\n\tbool \"a\"\n");
    write(&root, "b/Kconfig", "config B\n\tbool \"b\"\n");
    let src = "source \"a/Kconfig\"\n\
               source \"*/Kconfig\"\n\
               source \"missing/Kconfig\"\n\
               source \"C\"\n\
               config C\n\
               \tdepends on C\n";
    write(&root, "Kconfig", src);
    let (index, path) = index_of(&root, "Kconfig");
    let goto = |line, col| definition::goto_definition(&index, &path, Position::new(line, col));
    let file = |rel: &str| {
        Location::new(
            Url::from_file_path(root.join(rel)).unwrap(),
            Range::default(),
        )
    };

    // The path starts at its opening quote and ends after its closing quote.
    for col in [7, 9, 17] {
        assert_eq!(
            goto(0, col),
            Some(GotoDefinitionResponse::Scalar(file("a/Kconfig")))
        );
    }
    assert_eq!(goto(0, 18), None);
    assert_eq!(
        goto(1, 9),
        Some(GotoDefinitionResponse::Array(vec![
            file("a/Kconfig"),
            file("b/Kconfig")
        ]))
    );
    assert_eq!(goto(2, 9), None);
    // A path is not a symbol, also if a symbol has its name.
    assert_eq!(goto(3, 8), None);
    let c = Location::new(
        Url::from_file_path(&path).unwrap(),
        Range::new(Position::new(4, 7), Position::new(4, 8)),
    );
    assert_eq!(goto(5, 12), Some(GotoDefinitionResponse::Scalar(c)));
    std::fs::remove_dir_all(&root).unwrap();
}
