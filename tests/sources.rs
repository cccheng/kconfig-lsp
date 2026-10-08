use std::path::{Path, PathBuf};

use kconfig_lsp::analysis::WorldIndex;
use kconfig_lsp::settings::Settings;
use kconfig_lsp::sources;

/// An empty directory for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("kconfig-lsp-sources-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Analyzes `first` in a workspace at `root`, then reads the files that
/// it sources. Returns the index and the named files, relative to `root`.
fn read_from(root: &Path, first: &str, settings: Settings) -> (WorldIndex, Vec<String>) {
    let mut index = WorldIndex::new();
    index.settings = settings;
    index.root = Some(root.to_path_buf());
    let path = root.join(first);
    let source = std::fs::read_to_string(&path).unwrap();
    index.analyze_file(&path, &source);
    let read = sources::read_sourced_files(&mut index, vec![path]);
    let mut read: Vec<String> = read
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    read.sort();
    (index, read)
}

fn zephyr() -> Settings {
    Settings {
        zephyr_extensions: true,
        ..Default::default()
    }
}

#[test]
fn sourced_files_are_read_also_if_their_names_do_not_match() {
    let root = temp_dir("chain");
    write(&root, "Kconfig", "source \"arch/arm/Kconfig\"\n");
    write(
        &root,
        "arch/arm/Kconfig",
        "config MMU\n\tbool \"mmu\"\n\nif !MMU\nsource \"arch/arm/Kconfig-nommu\"\nendif\n",
    );
    write(
        &root,
        "arch/arm/Kconfig-nommu",
        "config ARM_MPU\n\tbool \"mpu\"\n",
    );

    let (index, read) = read_from(&root, "Kconfig", Settings::default());
    assert_eq!(read, ["arch/arm/Kconfig", "arch/arm/Kconfig-nommu"]);
    assert_eq!(index.get_definitions("ARM_MPU").len(), 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn each_file_is_read_once() {
    let root = temp_dir("cycle");
    write(
        &root,
        "Kconfig",
        "source \"a/Kconfig\"\nsource \"a/Kconfig\"\n",
    );
    write(
        &root,
        "a/Kconfig",
        "source \"Kconfig\"\nsource \"a/../a/Kconfig\"\n",
    );

    // `Kconfig` was in the index already, but it is named too.
    let (_, read) = read_from(&root, "Kconfig", Settings::default());
    assert_eq!(read, ["Kconfig", "a/Kconfig"]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn named_file_in_the_index_is_not_read_again() {
    let root = temp_dir("open");
    write(&root, "Kconfig", "source \"Config.in\"\n");
    write(&root, "Config.in", "config ON_DISK\n\tbool\n");

    // An open file has text that is not on disk.
    let mut index = WorldIndex::new();
    index.root = Some(root.clone());
    index.analyze_file(&root.join("Config.in"), "config OPEN\n\tbool\n");
    let path = root.join("Kconfig");
    index.analyze_file(&path, "source \"Config.in\"\n");
    let named = sources::read_sourced_files(&mut index, vec![path]);
    assert_eq!(named, [root.join("Config.in")]);
    assert_eq!(index.get_definitions("OPEN").len(), 1);
    assert!(index.get_definitions("ON_DISK").is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn dot_dot_needs_the_directory_before_it() {
    let root = temp_dir("dotdot");
    write(&root, "Kconfig", "source \"missing/../a/Kconfig\"\n");
    write(&root, "a/Kconfig", "config A\n\tbool\n");

    let (_, read) = read_from(&root, "Kconfig", Settings::default());
    assert!(read.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn star_matches_a_name_that_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt;

    let root = temp_dir("utf8");
    write(&root, "Kconfig", "source \"*/Kconfig\"\n");
    let dir = root.join(std::ffi::OsStr::from_bytes(b"d\xff"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Kconfig"), "config A\n\tbool\n").unwrap();

    let (index, read) = read_from(&root, "Kconfig", Settings::default());
    assert_eq!(read.len(), 1);
    assert_eq!(index.get_definitions("A").len(), 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn files_outside_the_workspace_are_not_read() {
    let base = temp_dir("outside");
    let root = base.join("root");
    write(&base, "outside/Kconfig", "config OUT\n\tbool\n");
    let absolute = base.join("outside/Kconfig");
    write(
        &root,
        "Kconfig",
        &format!(
            "source \"../outside/Kconfig\"\nsource \"{}\"\nsource \"missing/Kconfig\"\n",
            absolute.display()
        ),
    );

    let (index, read) = read_from(&root, "Kconfig", Settings::default());
    assert!(read.is_empty());
    assert!(index.get_definitions("OUT").is_empty());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn rsource_paths_start_at_the_file() {
    let root = temp_dir("rsource");
    write(&root, "x/Kconfig", "config TOP_X\n\tbool\n");
    write(&root, "sub/x/Kconfig", "config SUB_X\n\tbool\n");
    write(&root, "sub/b/Kconfig", "");
    write(&root, "sub/a/Kconfig", "");
    write(&root, "sub/.hidden/Kconfig", "");
    write(
        &root,
        "sub/Kconfig",
        "source \"x/Kconfig\"\nrsource \"x/Kconfig\"\nrsource \"*/Kconfig\"\norsource \"missing/Kconfig\"\n",
    );

    let (_, read) = read_from(&root, "sub/Kconfig", zephyr());
    assert_eq!(
        read,
        [
            "sub/a/Kconfig",
            "sub/b/Kconfig",
            "sub/x/Kconfig",
            "x/Kconfig"
        ]
    );

    let entries: Vec<String> = {
        let mut index = WorldIndex::new();
        index.settings = zephyr();
        let path = root.join("sub/Kconfig");
        index.analyze_file(&path, &std::fs::read_to_string(&path).unwrap());
        let fa = &index.files[&path];
        sources::source_entries(&fa.file)
            .iter()
            .map(|e| {
                // Glob results are sorted.
                let files = sources::resolve(e, &path, &fa.file.variables, Some(&root));
                files
                    .iter()
                    .map(|f| {
                        f.strip_prefix(&root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    };
    assert_eq!(
        entries,
        [
            "x/Kconfig",
            "sub/x/Kconfig",
            "sub/a/Kconfig sub/b/Kconfig sub/x/Kconfig",
            ""
        ]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn macro_variables_of_the_file_give_the_path() {
    let root = temp_dir("variables");
    write(&root, "Kconfig", "config RTT\n\tbool\n");
    write(&root, "bsp/board/drivers/Kconfig", "");
    write(&root, "bsp/board/packages/Kconfig", "");
    // An RT-Thread board gives its paths from its own directory.
    write(
        &root,
        "bsp/board/Kconfig",
        "BSP_DIR := .\nRTT_DIR := ../..\nPKGS_DIR := packages\n\
         source \"$(RTT_DIR)/Kconfig\"\nosource \"$PKGS_DIR/Kconfig\"\n\
         source \"$(BSP_DIR)/drivers/Kconfig\"\n",
    );

    let (_, read) = read_from(&root, "bsp/board/Kconfig", zephyr());
    assert_eq!(
        read,
        [
            "Kconfig",
            "bsp/board/drivers/Kconfig",
            "bsp/board/packages/Kconfig"
        ]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn paths_with_unknown_values_are_not_read() {
    let root = temp_dir("unknown");
    write(&root, "a/Kconfig", "");
    write(
        &root,
        "Kconfig",
        "source \"$(shell,echo a)/Kconfig\"\nsource \"$(KCONFIG_LSP_NO_SUCH_VAR)/Kconfig\"\n",
    );

    let (_, read) = read_from(&root, "Kconfig", Settings::default());
    assert!(read.is_empty());
    std::fs::remove_dir_all(&root).unwrap();
}
