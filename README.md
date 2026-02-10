# kconfig-lsp

A language server for the Kconfig configuration language used in Linux, Zephyr, U-Boot, coreboot, and other projects.

## Features

| LSP Method | Description |
|---|---|
| `textDocument/hover` | Keyword documentation and symbol help text |
| `textDocument/definition` | Jump to `config` / `menuconfig` definition |
| `textDocument/references` | Find all references to a symbol |
| `textDocument/completion` | Complete keywords and known symbols |
| `textDocument/publishDiagnostics` | Parse errors and undefined symbol warnings |

Full coverage of the Kconfig grammar defined in `Documentation/kbuild/kconfig-language.rst`:

- All entry types: `config`, `menuconfig`, `choice`, `comment`, `menu`, `if`, `source`, `mainmenu`
- All attributes: `bool`, `tristate`, `string`, `hex`, `int`, `prompt`, `default`, `def_bool`, `def_tristate`, `depends on`, `select`, `imply`, `visible if`, `range`, `help`, `modules`, `transitional`, `optional`
- Full expression syntax with correct precedence: `||`, `&&`, `=`, `!=`, `<`, `>`, `<=`, `>=`, `!`, `()`
- Macro invocations `$(...)`
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

### VS Code

Use a generic LSP client extension (e.g. [vscode-languageclient](https://github.com/AshutoshPandey-Devops/vscode-languageclient)) with the following settings:

```json
{
  "languageserver.kconfig.command": "kconfig-lsp",
  "languageserver.kconfig.filetypes": ["kconfig"]
}
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
| Macros | `$(cc-option,...)` `$(success,...)` |

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

## License

MIT
