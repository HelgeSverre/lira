import { useEditorStore } from '../../stores/editorStore';
import { CompileButton } from './CompileButton';
import { RunButton } from './RunButton';
import { DebugControls } from './DebugControls';
import { SampleSelector } from './SampleSelector';
import './Header.css';

export function Header() {
  const { isDirty } = useEditorStore();

  return (
    <header className="header">
      <div className="header-left">
        <div className="logo">
          <img src="/lira.svg" alt="Lira" className="logo-icon" />
          <span className="logo-text">Lira Playground</span>
        </div>
      </div>

      <div className="header-center">
        <SampleSelector />
      </div>

      <div className="header-right">
        {isDirty && <span className="unsaved-indicator">Modified</span>}
        <CompileButton />
        <RunButton />
        <div className="header-divider" />
        <DebugControls />
      </div>
    </header>
  );
}
