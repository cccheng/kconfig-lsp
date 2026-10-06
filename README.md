# kconfig-lsp

A language server for the Kconfig configuration language used in Linux, Zephyr, U-Boot, coreboot, and other projects.

## Features

| LSP Method | Description |
|---|---|
| `textDocument/hover` | Keyword documentation and symbol help text |
| `textDocument/definition` | Jump to `config` / `menuconfig` definition and `configdefault` blocks |
| `textDocument/references` | Find all references to a symbol |
| `textDocument/completion` | Complete keywords and known symbols |
| `textDocument/documentSymbol` | Outline of menus, choices, `if` blocks and symbols |
| `textDocument/publishDiagnostics` | Parse errors and undefined symbol warnings |

Full coverage of the Kconfig grammar defined in `Documentation/kbuild/kconfig-language.rst`:

- All entry types: `config`, `menuconfig`, `choice`, `comment`, `menu`, `if`, `source`, `mainmenu`
- All attributes: `bool`, `tristate`, `string`, `hex`, `int`, `prompt`, `default`, `def_bool`, `def_tristate`, `depends on`, `select`, `imply`, `visible if`, `range`, `help`, `modules`, `transitional`, `optional`
- Full expression syntax with correct precedence: `||`, `&&`, `=`, `!=`, `<`, `>`, `<=`, `>=`, `!`, `()`
- Macro invocations `$(...)` and macro variable assignments with `=`, `:=` and `+=`
- Line continuations `\`

## Installation

### From crates.io

```sh
cargo install kconfig-lsp
```

### From source

```sh
cargo build --release
cp target/release/kconfig-lsp ~/.local/bin/
```

## Editor Configuration

### Neovim

```lua
vim.lsp.config.kconfig = {
    root_markers = { '.git', 'Kconfig' },
    cmd = { 'kconfig-lsp' },
    filetypes = { 'kconfig' },
}

vim.lsp.enable('kconfig')
```

### Other Editors

Any LSP client that communicates over stdio can launch `kconfig-lsp` directly:

```sh
kconfig-lsp
```

## Supported Kconfig Syntax

| Category | Tokens |
|---|---|
| Entry keywords | `config` `menuconfig` `choice` `endchoice` `comment` `menu` `endmenu` `if` `endif` `source` `mainmenu` |
| Type keywords | `bool` `tristate` `string` `hex` `int` |
| Attribute keywords | `prompt` `default` `def_bool` `def_tristate` `depends` `on` `select` `imply` `visible` `range` `help` `modules` `transitional` `optional` |
| Operators | `=` `!=` `<` `>` `<=` `>=` `!` `&&` `\|\|` `(` `)` |
| Literals | `"double quoted"` `'single quoted'` |
| Macros | `$(cc-option,...)` `$(success,...)` `name = value` `name := value` `name += value` |
| Zephyr extensions (with `zephyr_extensions`) | `configdefault` `def_int` `def_hex` `def_string` |

## Configuration

Options are read from `initializationOptions` when the server starts, so
changing them requires restarting the server. Unknown options and values of
the wrong type are reported as a warning.

| option | type | default value | description |
|---|---|---|---|
| `zephyr_extensions` | bool | false | accept `configdefault`, `def_int`, `def_hex` and `def_string` from the [Zephyr Kconfig extensions](https://docs.zephyrproject.org/latest/build/kconfig/extensions.html) |

Neovim passes them through `init_options`:

```lua
vim.lsp.config.kconfig = {
    root_markers = { '.git', 'Kconfig' },
    cmd = { 'kconfig-lsp' },
    filetypes = { 'kconfig' },
    init_options = { zephyr_extensions = true },
}
```

## Building & Testing

```sh
cargo build
cargo test
```

The test suite includes:

- Lexer coverage for all keyword and operator tokens
- Parser correctness against a comprehensive Kconfig sample
- Semantic analysis: symbol definitions, references, type tracking
- Help text indentation parsing
- Real-world validation against the Linux kernel's `init/Kconfig`

The kernel test needs a Linux source tree and is skipped by default:

```sh
KCONFIG_LINUX_DIR=/path/to/linux cargo test -- --ignored
```

## License

MIT
