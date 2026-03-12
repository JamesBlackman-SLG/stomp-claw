import { SessionSidebar } from '../components/SessionSidebar'
import { ChatView } from '../components/ChatView'
import { StatusBar } from '../components/StatusBar'
import { TextInput } from '../components/TextInput'
import { ContextBar } from '../components/ContextBar'
import { ConnectionStatus } from '../components/ConnectionStatus'
import { HelpModal } from '../components/HelpModal'
import { useAppState, useDispatch } from '../lib/state'

export function Home() {
  const { showHelp } = useAppState()
  const dispatch = useDispatch()

  return (
    <>
      <header className="flex items-center justify-between px-3 sm:px-4 py-2 border-b border-border">
        <div className="flex items-center gap-2 sm:gap-2.5">
          <button
            onClick={() => dispatch({ type: 'set_sidebar_open', open: true })}
            className="md:hidden text-text-dim hover:text-accent transition-colors p-1"
            title="Sessions"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <line x1="3" y1="6" x2="21" y2="6" />
              <line x1="3" y1="12" x2="21" y2="12" />
              <line x1="3" y1="18" x2="21" y2="18" />
            </svg>
          </button>
          <img src="/logo.png" alt="StompClaw" className="h-7 w-7 sm:h-9 sm:w-9" />
          <h1 className="text-accent font-bold text-base sm:text-lg tracking-wider">STOMP CLAW</h1>
        </div>
        <div className="flex items-center gap-2 sm:gap-3">
          <button
            onClick={() => dispatch({ type: 'set_show_help', show: true })}
            className="text-text-dim hover:text-accent text-xs border border-border rounded-full px-2 sm:px-2.5 py-0.5 hover:border-accent transition-colors"
          >
            help
          </button>
          <ConnectionStatus />
        </div>
      </header>
      <div className="flex flex-1 overflow-hidden">
        <SessionSidebar />
        <main className="flex-1 flex flex-col min-w-0">
          <StatusBar />
          <ChatView />
          <TextInput />
          <ContextBar />
        </main>
      </div>
      {showHelp && <HelpModal onClose={() => dispatch({ type: 'set_show_help', show: false })} />}
    </>
  )
}
