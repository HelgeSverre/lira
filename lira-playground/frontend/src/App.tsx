import { Header } from './components/Header/Header'
import { PanelLayout } from './components/Layout/PanelLayout'
import { StatusBar } from './components/StatusBar/StatusBar'
import { EditorPanel } from './components/Editor/EditorPanel'
import { AstPanel } from './components/Ast/AstPanel'
import { OutputPanel } from './components/Output/OutputPanel'

function App() {
  return (
    <div className="app">
      <Header />
      <main className="main-content">
        <PanelLayout
          editor={<EditorPanel />}
          ast={<AstPanel />}
          output={<OutputPanel />}
        />
      </main>
      <StatusBar />
    </div>
  )
}

export default App
