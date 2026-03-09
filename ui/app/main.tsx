import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { AppProvider } from './lib/state'
import { Home } from './routes/index'
import './styles/app.css'

if ('serviceWorker' in navigator && (location.protocol === 'https:' || location.hostname === 'localhost' || location.hostname === '127.0.0.1')) {
  navigator.serviceWorker.register('/sw.js').catch(() => {})
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppProvider>
      <div className="h-dvh flex flex-col bg-bg text-text font-mono">
        <Home />
      </div>
    </AppProvider>
  </StrictMode>,
)
