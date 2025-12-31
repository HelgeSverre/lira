# vim-lira

Vim/Neovim syntax highlighting for the Lira programming language.

## Installation

### Using vim-plug

```vim
Plug 'lira-lang/vim-lira'
```

### Using Vundle

```vim
Plugin 'lira-lang/vim-lira'
```

### Using Pathogen

```bash
cd ~/.vim/bundle
git clone https://github.com/lira-lang/vim-lira.git
```

### Manual Installation

Copy the contents to your Vim configuration:

```bash
mkdir -p ~/.vim/ftdetect ~/.vim/ftplugin ~/.vim/syntax
cp ftdetect/lira.vim ~/.vim/ftdetect/
cp ftplugin/lira.vim ~/.vim/ftplugin/
cp syntax/lira.vim ~/.vim/syntax/
```

For Neovim, use `~/.config/nvim/` instead of `~/.vim/`.

## Features

- Syntax highlighting for `.li` and `.lira` files
- Proper indentation settings
- Comment string support for commenting plugins

## Highlighted Elements

- **Keywords**: `fn`, `let`, `var`, `const`, `struct`, `class`, `enum`, `trait`, `impl`, etc.
- **Control Flow**: `if`, `else`, `match`, `while`, `for`, `loop`, `break`, `continue`, `return`
- **Concurrency**: `spawn`, `select`, `async`
- **Types**: `int`, `float`, `bool`, `string`, `List`, `Map`, `Option`, `Result`, etc.
- **Literals**: Numbers, strings, characters, booleans
- **Comments**: Line (`//`) and block (`/* */`) comments
- **String Interpolation**: `${expression}`

## LSP Support

For full IDE features (completion, diagnostics, go-to-definition), install the Lira language server:

```bash
# Install lira-lsp
cargo install --path crates/lira-lsp

# Configure with nvim-lspconfig
```

Example Neovim LSP configuration:

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.lira then
  configs.lira = {
    default_config = {
      cmd = { 'lira-lsp' },
      filetypes = { 'lira' },
      root_dir = lspconfig.util.find_git_ancestor,
      settings = {},
    },
  }
end

lspconfig.lira.setup{}
```

## License

MIT
