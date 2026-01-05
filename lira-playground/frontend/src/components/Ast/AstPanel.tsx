import { useState } from 'react';
import { useCompilerStore } from '../../stores/compilerStore';
import { AstTreeView } from './AstTreeView';
import './AstPanel.css';

export function AstPanel() {
  const { ast, status, errors } = useCompilerStore();
  const [copied, setCopied] = useState(false);

  const handleCopyJson = async () => {
    if (!ast) return;
    await navigator.clipboard.writeText(JSON.stringify(ast, null, 2));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="panel ast-panel" data-testid="ast-panel" data-status={status}>
      <div className="panel-header">
        <span className="ast-title">AST</span>
        {ast && (
          <>
            <span className="node-count" data-testid="ast-statement-count">
              {ast.statements.length} statements
            </span>
            <button
              className="copy-json-btn"
              onClick={handleCopyJson}
              title="Copy AST as JSON"
              data-testid="copy-ast-json"
            >
              {copied ? 'Copied!' : 'Copy JSON'}
            </button>
          </>
        )}
      </div>
      <div className="panel-content ast-content" data-testid="ast-content">
        {status === 'idle' && (
          <div className="ast-placeholder" data-testid="ast-placeholder">
            Click "Compile" to see the AST
          </div>
        )}
        {status === 'compiling' && (
          <div className="ast-placeholder" data-testid="ast-compiling">
            Compiling...
          </div>
        )}
        {status === 'error' && (
          <div className="ast-error" data-testid="ast-errors">
            <div className="error-title">Compilation Errors</div>
            {errors.map((error, i) => (
              <div key={i} className="error-item" data-testid="ast-error">
                {error.line && <span className="error-location">Line {error.line}: </span>}
                <span className="error-message">{error.message}</span>
              </div>
            ))}
          </div>
        )}
        {status === 'success' && ast && (
          <AstTreeView program={ast} />
        )}
      </div>
    </div>
  );
}
