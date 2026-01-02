import { MonacoEditor } from './MonacoEditor';
import './EditorPanel.css';

export function EditorPanel() {
  return (
    <div className="panel editor-panel">
      <div className="panel-header">
        <span>Editor</span>
      </div>
      <div className="panel-content editor-content">
        <MonacoEditor />
      </div>
    </div>
  );
}
