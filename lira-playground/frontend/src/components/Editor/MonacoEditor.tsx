import { useRef, useEffect, useCallback } from 'react';
import Editor, { OnMount, BeforeMount, Monaco } from '@monaco-editor/react';
import { useEditorStore } from '../../stores/editorStore';
import { useCompilerStore } from '../../stores/compilerStore';
import { useUiStore } from '../../stores/uiStore';
import { registerLiraLanguage, defineLiraTheme } from './liraLanguage';
import './MonacoEditor.css';

export function MonacoEditor() {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const editorRef = useRef<any>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const decorationsRef = useRef<string[]>([]);

  const {
    sourceCode,
    setSourceCode,
    breakpoints,
    toggleBreakpoint,
    setCursorPosition,
    highlightedLine,
  } = useEditorStore();

  const { errors } = useCompilerStore();
  const { fontSize } = useUiStore();

  // Handle Monaco setup before mount
  const handleBeforeMount: BeforeMount = (monaco) => {
    monacoRef.current = monaco;
    registerLiraLanguage(monaco);
    defineLiraTheme(monaco);
  };

  // Handle editor mount
  const handleMount: OnMount = (editor, monaco) => {
    editorRef.current = editor;

    // Track cursor position
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    editor.onDidChangeCursorPosition((e: any) => {
      setCursorPosition(e.position.lineNumber, e.position.column);
    });

    // Handle breakpoint clicks in gutter
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    editor.onMouseDown((e: any) => {
      if (
        e.target.type === monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN ||
        e.target.type === monaco.editor.MouseTargetType.GUTTER_LINE_NUMBERS
      ) {
        const line = e.target.position?.lineNumber;
        if (line) {
          toggleBreakpoint(line);
        }
      }
    });

    // Focus editor
    editor.focus();
  };

  // Handle content change
  const handleChange = useCallback((value: string | undefined) => {
    if (value !== undefined) {
      setSourceCode(value);
    }
  }, [setSourceCode]);

  // Update decorations when breakpoints or highlighted line changes
  useEffect(() => {
    const editorInstance = editorRef.current;
    const monaco = monacoRef.current;
    if (!editorInstance || !monaco) return;

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const newDecorations: any[] = [];

    // Breakpoint decorations
    for (const line of breakpoints) {
      newDecorations.push({
        range: new monaco.Range(line, 1, line, 1),
        options: {
          isWholeLine: false,
          glyphMarginClassName: 'breakpoint-glyph',
          glyphMarginHoverMessage: { value: 'Breakpoint' },
        },
      });
    }

    // Current execution line decoration
    if (highlightedLine !== null) {
      newDecorations.push({
        range: new monaco.Range(highlightedLine, 1, highlightedLine, 1),
        options: {
          isWholeLine: true,
          className: 'current-execution-line',
          glyphMarginClassName: 'current-execution-glyph',
        },
      });
    }

    decorationsRef.current = editorInstance.deltaDecorations(
      decorationsRef.current,
      newDecorations
    );
  }, [breakpoints, highlightedLine]);

  // Update error markers
  useEffect(() => {
    const monaco = monacoRef.current;
    const editorInstance = editorRef.current;
    if (!monaco || !editorInstance) return;

    const model = editorInstance.getModel();
    if (!model) return;

    const markers = errors
      .filter((error) => error.line !== null)
      .map((error) => ({
        severity: error.severity === 'error'
          ? monaco.MarkerSeverity.Error
          : error.severity === 'warning'
          ? monaco.MarkerSeverity.Warning
          : monaco.MarkerSeverity.Hint,
        startLineNumber: error.line ?? 1,
        startColumn: error.column ?? 1,
        endLineNumber: error.line ?? 1,
        endColumn: (error.column ?? 1) + 10,
        message: error.message,
        source: 'lirac',
      }));

    monaco.editor.setModelMarkers(model, 'lirac', markers);
  }, [errors]);

  return (
    <div className="monaco-editor-wrapper">
      <Editor
        language="lira"
        theme="lira-dark"
        value={sourceCode}
        onChange={handleChange}
        beforeMount={handleBeforeMount}
        onMount={handleMount}
        options={{
          fontSize,
          fontFamily: "'Menlo', 'Monaco', 'Consolas', monospace",
          minimap: { enabled: false },
          lineNumbers: 'on',
          glyphMargin: true,
          folding: true,
          lineDecorationsWidth: 10,
          lineNumbersMinChars: 3,
          renderLineHighlight: 'all',
          scrollBeyondLastLine: false,
          automaticLayout: true,
          tabSize: 4,
          insertSpaces: true,
          wordWrap: 'off',
          contextmenu: true,
          quickSuggestions: true,
          suggestOnTriggerCharacters: true,
          acceptSuggestionOnEnter: 'on',
          cursorBlinking: 'blink',
          cursorStyle: 'line',
          smoothScrolling: true,
        }}
      />
    </div>
  );
}
