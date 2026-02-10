use std::path::Path;

use tower_lsp::lsp_types::*;

use crate::analysis::WorldIndex;

pub fn hover(index: &WorldIndex, path: &Path, pos: Position) -> Option<Hover> {
    let fa = index.files.get(path)?;
    let offset = fa.line_index.offset(pos.line, pos.character);
    let word = word_at_offset(&fa.source, offset)?;

    if let Some(doc) = keyword_docs(&word) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            }),
            range: None,
        });
    }

    let defs = index.get_definitions(&word);
    if !defs.is_empty() {
        let mut parts: Vec<String> = Vec::new();
        for d in defs {
            let mut section = format!("**{}** ({})", d.name, def_kind_label(d.kind));
            if let Some(tk) = d.type_kind {
                section.push_str(&format!(" `{}`", tk.as_str()));
            }
            if let Some(prompt) = &d.prompt {
                section.push_str(&format!("\n\n*\"{}\"*", prompt));
            }
            section.push_str(&format!("\n\nDefined in `{}`", d.file.display()));
            if let Some(help) = &d.help {
                section.push_str(&format!("\n\n---\n\n{}", help));
            }
            parts.push(section);
        }
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: parts.join("\n\n---\n\n"),
            }),
            range: None,
        });
    }

    None
}

fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return None;
    }

    let mut start = offset;
    while start > 0 && is_word_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && is_word_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(source[start..end].to_string())
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn def_kind_label(kind: crate::analysis::DefKind) -> &'static str {
    match kind {
        crate::analysis::DefKind::Config => "config",
        crate::analysis::DefKind::MenuConfig => "menuconfig",
        crate::analysis::DefKind::Choice => "choice",
    }
}

fn keyword_docs(word: &str) -> Option<&'static str> {
    Some(match word {
        "config" => {
            "\
**config** `<symbol>`

Starts a new config entry. The following lines define attributes for \
this config option. Attributes can be the type of the config option, \
input prompt, dependencies, help text and default values. A config \
option can be defined multiple times with the same name, but every \
definition can have only a single input prompt and the type must not \
conflict."
        }

        "menuconfig" => {
            "\
**menuconfig** `<symbol>`

Similar to `config`, but also gives a hint to front ends that all \
sub-options should be displayed as a separate list of options. To make \
sure all the sub-options will really show up under the menuconfig entry \
and not outside of it, every item from the options list must depend on \
the menuconfig symbol.

```kconfig
menuconfig M
if M
    config C1
    config C2
endif
```"
        }

        "choice" | "endchoice" => {
            "\
**choice** / **endchoice**

Defines a choice group. A choice allows only a single config entry \
to be selected. Accepts `prompt`, `default`, `depends on`, and `help` \
attributes.

```kconfig
choice
    prompt \"Choose one\"
    default OPT_A

config OPT_A
    bool \"Option A\"

config OPT_B
    bool \"Option B\"

endchoice
```"
        }

        "comment" => {
            "\
**comment** `<prompt>`

Defines a comment which is displayed to the user during the \
configuration process and is also echoed to the output files. \
The only possible options are dependencies."
        }

        "menu" | "endmenu" => {
            "\
**menu** `<prompt>` / **endmenu**

Defines a menu block. All entries within the `menu` ... `endmenu` \
block become a submenu. All sub-entries inherit the dependencies \
from the menu entry. The only possible options are dependencies \
and `visible` attributes."
        }

        "if" | "endif" => {
            "\
**if** `<expr>` / **endif**

Defines an if block. The dependency expression is appended to all \
enclosed menu entries."
        }

        "source" => {
            "\
**source** `<path>`

Reads the specified configuration file. This file is always parsed."
        }

        "mainmenu" => {
            "\
**mainmenu** `<prompt>`

Sets the config program's title bar. It should be placed at the top \
of the configuration, before any other statement."
        }

        "bool" => {
            "\
**bool** [`<prompt>`]

Boolean type. The config option can be `y` (built-in) or `n` (disabled)."
        }

        "tristate" => {
            "\
**tristate** [`<prompt>`]

Tristate type. The config option can be `y` (built-in), `m` (module), \
or `n` (disabled)."
        }

        "string" => {
            "\
**string** [`<prompt>`]

String type. The config option holds a free-form string value."
        }

        "hex" => {
            "\
**hex** [`<prompt>`]

Hexadecimal type. The config option holds a hex value (e.g. `0x1234`)."
        }

        "int" => {
            "\
**int** [`<prompt>`]

Integer type. The config option holds a decimal integer value."
        }

        "prompt" => {
            "\
**prompt** `<prompt>` [`if` `<expr>`]

Sets the input prompt displayed to the user. Every menu entry can have \
at most one prompt. Optionally, a dependency for this prompt can be \
added with `if`."
        }

        "default" => {
            "\
**default** `<expr>` [`if` `<expr>`]

Sets a default value. If multiple default values are visible, only the \
first defined one is active. Default values are not limited to the menu \
entry where they are defined.

The default value deliberately defaults to `n` in order to avoid \
bloating the build. With few exceptions, new config options should not \
change this."
        }

        "def_bool" => {
            "\
**def_bool** `<expr>` [`if` `<expr>`]

Shorthand for a `bool` type definition plus a default value."
        }

        "def_tristate" => {
            "\
**def_tristate** `<expr>` [`if` `<expr>`]

Shorthand for a `tristate` type definition plus a default value."
        }

        "depends" => {
            "\
**depends on** `<expr>`

Defines a dependency for this menu entry. If multiple dependencies \
are defined, they are connected with `&&`. Dependencies are applied \
to all other options within this menu entry."
        }

        "select" => {
            "\
**select** `<symbol>` [`if` `<expr>`]

Reverse dependency. Forces a lower limit on another symbol. The value \
of the current menu symbol is used as the minimal value the selected \
symbol can be set to.

**Note:** `select` should be used with care. It will force a symbol \
to a value without visiting the dependencies. In general use `select` \
only for non-visible symbols (no prompts) and for symbols with no \
dependencies."
        }

        "imply" => {
            "\
**imply** `<symbol>` [`if` `<expr>`]

Weak reverse dependency. Similar to `select` but the implied symbol's \
value may still be set to `n` from a direct dependency or with a \
visible prompt."
        }

        "visible" => {
            "\
**visible if** `<expr>`

Only applicable to menu blocks. If the condition is false, the menu \
block is not displayed to the user (the symbols contained there can \
still be selected by other symbols, though). Default value is `true`."
        }

        "range" => {
            "\
**range** `<symbol>` `<symbol>` [`if` `<expr>`]

Limits the range of possible input values for `int` and `hex` symbols. \
The user can only input a value which is `>=` the first symbol and \
`<=` the second symbol."
        }

        "help" => {
            "\
**help**

Defines a help text. The end of the help text is determined by the \
indentation level — it ends at the first line which has a smaller \
indentation than the first line of the help text.

Per kernel coding style: help text is indented with one tab plus two \
additional spaces."
        }

        "modules" => {
            "\
**modules**

Declares the symbol to be used as the `MODULES` symbol, which enables \
the third modular state for all config symbols. At most one symbol may \
have the `modules` option set."
        }

        "transitional" => {
            "\
**transitional**

Declares the symbol as transitional, meaning it should be processed \
during configuration but omitted from newly written `.config` files. \
Useful for backward compatibility during config option migrations.

A transitional symbol has no prompt, is not written to new `.config` \
files, and cannot have any other properties."
        }

        "optional" => {
            "\
**optional**

Marks a choice as optional — the user may leave all options unselected."
        }

        "on" => {
            "\
Part of the **depends on** syntax. See `depends`."
        }

        _ => return None,
    })
}
