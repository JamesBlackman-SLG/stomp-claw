import { SessionSidebar } from '../components/SessionSidebar'
import { ChatView } from '../components/ChatView'
import { StatusBar } from '../components/StatusBar'
import { TextInput } from '../components/TextInput'
import { ConnectionStatus } from '../components/ConnectionStatus'
import { HelpModal } from '../components/HelpModal'
import { useAppState, useDispatch } from '../lib/state'

export function Home() {
  const { showHelp } = useAppState()
  const dispatch = useDispatch()

  return (
    <>
      <header className="flex items-center justify-between px-4 py-2 border-b border-border">
        <div className="flex items-center gap-2.5">
          <img src="/logo.png" alt="StompClaw" className="h-9 w-9" />
          <h1 className="text-accent font-bold text-lg tracking-wider">StompClaw</h1>
        </div>
        <div className="flex items-center gap-3">
          <button
            onClick={() => dispatch({ type: 'set_show_help', show: true })}
            className="text-text-dim hover:text-accent text-xs border border-border rounded-full px-2.5 py-0.5 hover:border-accent transition-colors"
          >
            help
          </button>
          <ConnectionStatus />
        </div>
      </header>
      <div className="flex flex-1 overflow-hidden">
        <SessionSidebar />
        <main className="flex-1 flex flex-col">
          <StatusBar />
          <ChatView />
          <TextInput />
        </main>
      </div>
      {showHelp && <HelpModal onClose={() => dispatch({ type: 'set_show_help', show: false })} />}
    </>
  )
}
