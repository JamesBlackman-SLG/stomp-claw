import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { AppProvider } from './lib/state'
import { Home } from './routes/index'
import './styles/app.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppProvider>
      <div className="h-dvh flex flex-col bg-bg text-text font-mono">
        <Home />
      </div>
    </AppProvider>
  </StrictMode>,
)
