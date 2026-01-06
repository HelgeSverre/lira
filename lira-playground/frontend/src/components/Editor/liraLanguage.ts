import type { Monaco } from '@monaco-editor/react';

/**
 * Register Lira language definition for Monaco Editor
 * Based on editors/vscode-lira/syntaxes/lira.tmLanguage.json
 */
export function registerLiraLanguage(monaco: Monaco): void {
  // Register language
  monaco.languages.register({
    id: 'lira',
    extensions: ['.li', '.lira'],
    aliases: ['Lira', 'lira'],
  });

  // Set token provider using Monarch syntax
  monaco.languages.setMonarchTokensProvider('lira', {
    keywords: [
      'fn', 'let', 'var', 'const', 'struct', 'class', 'enum', 'trait',
      'interface', 'impl', 'type', 'pub', 'priv', 'static', 'mut', 'where',
      'if', 'else', 'match', 'while', 'for', 'loop', 'in', 'break',
      'continue', 'return', 'spawn', 'select', 'async', 'import', 'use',
      'try', 'catch', 'finally', 'as', 'is', 'new', 'extends', 'abstract', 'override'
    ],

    typeKeywords: [
      'int', 'int8', 'int16', 'int32', 'int64',
      'uint8', 'uint16', 'uint32', 'uint64',
      'float', 'bool', 'string', 'char', 'void',
      'List', 'Map', 'Set', 'Option', 'Result', 'Channel'
    ],

    constants: ['true', 'false', 'null'],

    builtins: [
      'print', 'println', 'debug', 'assert', 'len', 'push', 'pop',
      'append', 'panic', 'todo', 'unreachable', 'chan', 'fiber_id',
      'fiber_yield', 'close', 'Ok', 'Err', 'Some', 'None'
    ],

    operators: [
      '=', '>', '<', '!', '~', '?', ':', '==', '<=', '>=', '!=',
      '&&', '||', '++', '--', '+', '-', '*', '/', '&', '|', '^', '%',
      '<<', '>>', '+=', '-=', '*=', '/=', '&=', '|=', '^=', '%=',
      '<<=', '>>=', '->', '=>', '<-', '??', '?.', '::'
    ],

    symbols: /[=><!~?:&|+\-*\/\^%]+/,

    escapes: /\\(?:[abfnrtv\\"'0]|x[0-9A-Fa-f]{2}|u[0-9A-Fa-f]{4})/,

    tokenizer: {
      root: [
        // Identifiers and keywords
        [/[a-z_$][\w$]*/, {
          cases: {
            '@keywords': 'keyword',
            '@typeKeywords': 'type',
            '@constants': 'constant',
            '@builtins': 'support.function',
            '@default': 'identifier'
          }
        }],

        // Type identifiers (capitalized)
        [/[A-Z][\w$]*/, 'type.identifier'],

        // Whitespace
        { include: '@whitespace' },

        // Delimiters
        [/[{}()\[\]]/, '@brackets'],
        [/[<>](?!@symbols)/, '@brackets'],
        [/@symbols/, {
          cases: {
            '@operators': 'operator',
            '@default': ''
          }
        }],

        // Comma and semicolon
        [/[,;]/, 'delimiter'],

        // Numbers
        [/\d*\.\d+([eE][\-+]?\d+)?/, 'number.float'],
        [/0[xX][0-9a-fA-F]+/, 'number.hex'],
        [/0[bB][01]+/, 'number.binary'],
        [/0[oO][0-7]+/, 'number.octal'],
        [/\d+/, 'number'],

        // Strings
        [/"([^"\\]|\\.)*$/, 'string.invalid'],
        [/"/, 'string', '@string'],

        // Characters
        [/'[^\\']'/, 'string.char'],
        [/'/, 'string.char', '@character'],
      ],

      whitespace: [
        [/[ \t\r\n]+/, ''],
        [/\/\/\/.*$/, 'comment.doc'],
        [/\/\/.*$/, 'comment'],
        [/\/\*/, 'comment', '@comment'],
      ],

      comment: [
        [/[^\/*]+/, 'comment'],
        [/\*\//, 'comment', '@pop'],
        [/[\/*]/, 'comment'],
      ],

      string: [
        [/[^\\"$]+/, 'string'],
        [/@escapes/, 'string.escape'],
        [/\\./, 'string.escape.invalid'],
        [/\$\{/, { token: 'delimiter.bracket', next: '@stringInterpolation' }],
        [/"/, 'string', '@pop'],
      ],

      stringInterpolation: [
        [/[^}]+/, 'identifier'],
        [/\}/, { token: 'delimiter.bracket', next: '@pop' }],
      ],

      character: [
        [/@escapes/, 'string.escape'],
        [/[^\\']/, 'string.char'],
        [/'/, 'string.char', '@pop'],
      ],
    },
  });

  // Set language configuration
  monaco.languages.setLanguageConfiguration('lira', {
    comments: {
      lineComment: '//',
      blockComment: ['/*', '*/'],
    },
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
      ['<', '>'],
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"', notIn: ['string'] },
      { open: "'", close: "'", notIn: ['string', 'comment'] },
      { open: '<', close: '>' },
    ],
    surroundingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '<', close: '>' },
    ],
    folding: {
      markers: {
        start: /^\s*\/\/\s*#?region\b/,
        end: /^\s*\/\/\s*#?endregion\b/,
      },
    },
    indentationRules: {
      increaseIndentPattern: /^.*\{[^}"']*$/,
      decreaseIndentPattern: /^\s*\}/,
    },
  });
}

/**
 * Define Lira Night theme colors for Monaco
 * Based on docs/branding.md color specifications
 */
export function defineLiraTheme(monaco: Monaco): void {
  // Lira Brand Colors
  const night = '1A1B26';
  const snow = 'F5F5F7';
  const liraGold = 'E8A924';
  const starlight = '6B9AC4';
  const moss = '7AA874';
  const dusk = '2D2F3D';
  const types = 'C4A7E7';
  const operators = '89DDFF';
  const commentGray = '6B7280';

  monaco.editor.defineTheme('lira-dark', {
    base: 'vs-dark',
    inherit: false,
    rules: [
      { token: '', foreground: snow, background: night },
      { token: 'keyword', foreground: liraGold, fontStyle: 'bold' },
      { token: 'type', foreground: types },
      { token: 'type.identifier', foreground: types },
      { token: 'constant', foreground: liraGold },
      { token: 'support.function', foreground: snow },
      { token: 'string', foreground: moss },
      { token: 'string.char', foreground: moss },
      { token: 'string.escape', foreground: starlight },
      { token: 'number', foreground: starlight },
      { token: 'number.float', foreground: starlight },
      { token: 'number.hex', foreground: starlight },
      { token: 'number.binary', foreground: starlight },
      { token: 'number.octal', foreground: starlight },
      { token: 'comment', foreground: commentGray, fontStyle: 'italic' },
      { token: 'comment.doc', foreground: commentGray, fontStyle: 'italic' },
      { token: 'operator', foreground: operators },
      { token: 'delimiter', foreground: snow },
      { token: 'identifier', foreground: snow },
    ],
    colors: {
      'editor.background': '#' + night,
      'editor.foreground': '#' + snow,
      'editor.lineHighlightBackground': '#' + dusk,
      'editor.lineHighlightBorder': '#00000000',
      'editorCursor.foreground': '#' + liraGold,
      'editor.selectionBackground': '#' + liraGold + '40',
      'editor.selectionHighlightBackground': '#' + liraGold + '20',
      'editorLineNumber.foreground': '#' + commentGray,
      'editorLineNumber.activeForeground': '#' + snow,
      'editorGutter.background': '#' + night,
      'editorIndentGuide.background': '#' + dusk,
      'editorIndentGuide.activeBackground': '#' + commentGray,
      'editorBracketMatch.background': '#' + liraGold + '30',
      'editorBracketMatch.border': '#' + liraGold,
    },
  });
}
