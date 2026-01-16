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

# Verify installation
lira-lsp --version
```

### Neovim with nvim-lspconfig

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

### Neovim with coc.nvim

Add to your `coc-settings.json` (`:CocConfig`):

```json
{
  "languageserver": {
    "lira": {
      "command": "lira-lsp",
      "filetypes": ["lira"],
      "rootPatterns": [".git/", "*.li"]
    }
  }
}
```

### Vim with ALE

Add to your `.vimrc`:

```vim
" Register lira-lsp with ALE
let g:ale_linters = {
\   'lira': ['lira-lsp'],
\}

" Define lira-lsp linter
call ale#linter#Define('lira', {
\   'name': 'lira-lsp',
\   'lsp': 'stdio',
\   'executable': 'lira-lsp',
\   'command': 'lira-lsp',
\   'project_root': function('ale#handlers#go#FindProjectRoot'),
\})
```

### Vim with vim-lsp

Add to your `.vimrc`:

```vim
if executable('lira-lsp')
  au User lsp_setup call lsp#register_server({
    \ 'name': 'lira-lsp',
    \ 'cmd': {server_info->['lira-lsp']},
    \ 'allowlist': ['lira'],
    \ })
endif
```

## Tree-sitter Support (Neovim)

For enhanced syntax highlighting with tree-sitter, add this to your Neovim config:

```lua
local parser_config = require("nvim-treesitter.parsers").get_parser_configs()

parser_config.lira = {
  install_info = {
    url = "https://github.com/HelgeSverre/lira",
    files = { "editors/tree-sitter-lira/src/parser.c" },
    branch = "main",
    generate_requires_npm = true,
  },
  filetype = "lira",
}

-- Associate .li files with lira filetype
vim.filetype.add({
  extension = {
    li = "lira",
    lira = "lira",
  },
})
```

Then install the parser:

```vim
:TSInstall lira
```

Copy query files for highlighting:

```bash
# Create queries directory
mkdir -p ~/.config/nvim/queries/lira

# Copy from lira repo
cp editors/tree-sitter-lira/queries/*.scm ~/.config/nvim/queries/lira/
```

## License

MIT
