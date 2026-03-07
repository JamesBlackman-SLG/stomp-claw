import { SessionSidebar } from '../components/SessionSidebar'
import { ChatView } from '../components/ChatView'
import { StatusBar } from '../components/StatusBar'
import { TextInput } from '../components/TextInput'
import { ConnectionStatus } from '../components/ConnectionStatus'

export function Home() {
  return (
    <>
      <header className="flex items-center justify-between px-4 py-2 border-b border-border">
        <h1 className="text-accent font-bold text-lg tracking-wider">STOMP CLAW</h1>
        <ConnectionStatus />
      </header>
      <div className="flex flex-1 overflow-hidden">
        <SessionSidebar />
        <main className="flex-1 flex flex-col">
          <StatusBar />
          <ChatView />
          <TextInput />
        </main>
      </div>
    </>
  )
}
