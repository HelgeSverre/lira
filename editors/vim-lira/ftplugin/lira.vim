" Vim filetype plugin for Lira
" Language: Lira
" Maintainer: Lira contributors

if exists("b:did_ftplugin")
  finish
endif
let b:did_ftplugin = 1

" Set comment string for commenting plugins
setlocal commentstring=//\ %s

" Indentation settings
setlocal tabstop=4
setlocal shiftwidth=4
setlocal softtabstop=4
setlocal expandtab

" Enable auto-indentation
setlocal autoindent
setlocal smartindent

" Undo settings when switching filetype
let b:undo_ftplugin = "setl com< cms< ts< sw< sts< et< ai< si<"
