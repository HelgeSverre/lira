import { useEditorStore } from '../../stores/editorStore';
import { useCompilerStore } from '../../stores/compilerStore';
import { mockCompile } from '../../mocks/mockCompiler';

export function CompileButton() {
  const { sourceCode, markClean } = useEditorStore();
  const { status, startCompilation, setCompilationSuccess, setCompilationError } = useCompilerStore();

  const isCompiling = status === 'compiling';

  const handleCompile = async () => {
    startCompilation();

    // Simulate async compilation
    await new Promise(resolve => setTimeout(resolve, 100));

    const result = mockCompile(sourceCode);

    if (result.errors.length > 0) {
      setCompilationError(result.errors);
    } else {
      setCompilationSuccess(result.ast);
      markClean();
    }
  };

  return (
    <button
      className="action-button primary"
      onClick={handleCompile}
      disabled={isCompiling}
      title="Compile (Ctrl+Enter)"
    >
      {isCompiling ? (
        <>
          <span className="spinner" />
          Compiling...
        </>
      ) : (
        <>
          <CheckIcon />
          Compile
        </>
      )}
    </button>
  );
}

function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
      <path d="M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z" />
    </svg>
  );
}
