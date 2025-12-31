" Vim syntax file for Lira
" Language: Lira
" Maintainer: Lira contributors

if exists("b:current_syntax")
  finish
endif

" Comments
syn match liraComment "//.*$" contains=liraTodo
syn region liraBlockComment start="/\*" end="\*/" contains=liraTodo,liraBlockComment
syn keyword liraTodo contained TODO FIXME XXX NOTE HACK BUG

" Doc comments
syn match liraDocComment "///.*$"
syn match liraDocComment "//!.*$"

" Keywords - Declaration
syn keyword liraKeyword fn let var const struct class enum trait interface impl type pub priv static

" Keywords - Control flow
syn keyword liraConditional if else match
syn keyword liraRepeat while for loop in
syn keyword liraControl break continue return

" Keywords - Concurrency
syn keyword liraConcurrency spawn select async

" Keywords - Error handling
syn keyword liraException try catch finally

" Keywords - Module
syn keyword liraInclude import use

" Keywords - OOP
syn keyword liraKeyword new extends abstract override

" Keywords - Modifiers
syn keyword liraModifier mut where

" Keywords - Operators as keywords
syn keyword liraOperatorKw as is

" Built-in types
syn keyword liraType int int8 int16 int32 int64 uint8 uint16 uint32 uint64
syn keyword liraType float bool string char void
syn keyword liraType List Map Set Option Result Channel

" Boolean literals
syn keyword liraBoolean true false

" Null literal
syn keyword liraConstant null

" Self/this/super
syn keyword liraSelf self this super Self

" Function names (after fn keyword)
syn match liraFunction "\<fn\s\+\zs[a-zA-Z_][a-zA-Z0-9_]*"

" Type names (PascalCase)
syn match liraTypeName "\<[A-Z][a-zA-Z0-9_]*\>"

" Numbers
syn match liraNumber "\<\d\+\>"
syn match liraNumber "\<0x[0-9a-fA-F]\+\>"
syn match liraNumber "\<0b[01]\+\>"
syn match liraNumber "\<0o[0-7]\+\>"
syn match liraFloat "\<\d\+\.\d*\>"
syn match liraFloat "\<\d*\.\d\+\>"
syn match liraFloat "\<\d\+[eE][+-]\?\d\+\>"

" Strings
syn region liraString start=+"+ end=+"+ skip=+\\"+ contains=liraEscape,liraInterpolation
syn region liraString start=+'+ end=+'+ skip=+\\'+ contains=liraEscape
syn region liraRawString start=+r"+ end=+"+ skip=+\\"+
syn region liraMultilineString start=+"""+ end=+"""+ contains=liraEscape,liraInterpolation

" Character literals
syn match liraChar "'.'"
syn match liraChar "'\\[nrt\\'\"]'"
syn match liraChar "'\\x[0-9a-fA-F]\{2\}'"
syn match liraChar "'\\u[0-9a-fA-F]\{4\}'"

" Escape sequences
syn match liraEscape contained "\\[nrt\\'\"\$0]"
syn match liraEscape contained "\\x[0-9a-fA-F]\{2\}"
syn match liraEscape contained "\\u[0-9a-fA-F]\{4\}"

" String interpolation
syn region liraInterpolation contained start="\${" end="}" contains=TOP

" Operators
syn match liraOperator "[+\-*/%]"
syn match liraOperator "[<>=!]"
syn match liraOperator "[&|^~]"
syn match liraOperator "\.\."
syn match liraOperator "\.\.="
syn match liraOperator "->"
syn match liraOperator "=>"
syn match liraOperator "<-"
syn match liraOperator "??"
syn match liraOperator "?."
syn match liraOperator "::"

" Delimiters
syn match liraDelimiter "[,;:]"
syn match liraBracket "[(){}\[\]]"

" Built-in functions
syn keyword liraBuiltin print println debug assert len push pop append panic todo unreachable

" Highlighting links
hi def link liraComment Comment
hi def link liraBlockComment Comment
hi def link liraDocComment SpecialComment
hi def link liraTodo Todo

hi def link liraKeyword Keyword
hi def link liraConditional Conditional
hi def link liraRepeat Repeat
hi def link liraControl Statement
hi def link liraConcurrency Keyword
hi def link liraException Exception
hi def link liraInclude Include
hi def link liraModifier StorageClass
hi def link liraOperatorKw Operator

hi def link liraType Type
hi def link liraTypeName Type
hi def link liraBoolean Boolean
hi def link liraConstant Constant
hi def link liraSelf Special

hi def link liraFunction Function
hi def link liraBuiltin Function

hi def link liraNumber Number
hi def link liraFloat Float
hi def link liraString String
hi def link liraRawString String
hi def link liraMultilineString String
hi def link liraChar Character
hi def link liraEscape SpecialChar
hi def link liraInterpolation Special

hi def link liraOperator Operator
hi def link liraDelimiter Delimiter
hi def link liraBracket Delimiter

let b:current_syntax = "lira"
